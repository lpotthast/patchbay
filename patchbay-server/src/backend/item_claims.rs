use std::collections::BTreeMap;

use crudkit_core::condition::Condition;
use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseTransaction,
    EntityTrait, QueryFilter, QueryOrder, Statement, TransactionTrait,
};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    backend::{
        entities::{
            comment::CommentModel,
            work_item::{self, WorkItem, WorkItemActiveModel, WorkItemModel},
        },
        events, item_labels, projects,
        storage::{Store, utc_now},
        work_item_comments, work_item_events, work_item_labels, work_items,
    },
    shared::view_models::{
        AUTOMATION_BLOCKED_LABEL_KEY, AuthorType, CLAIMED_FROM_STATE_LABEL_KEY,
        CLAIMED_STATE_LABEL, CommentView, FEEDBACK_REQUESTED_LABEL_KEY, FINISHED_STATE_LABEL,
        RecoveredClaimView, STATE_LABEL_KEY, WorkItemLabelView, WorkItemView,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseAutomationDisposition {
    Claimable,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClaimReturnMode<'a> {
    Release {
        comment: Option<&'a str>,
        automation_disposition: ReleaseAutomationDisposition,
    },
    FeedbackRequest {
        body: &'a str,
    },
}

impl ClaimReturnMode<'_> {
    fn agent_comment_body(&self) -> Option<&str> {
        match self {
            Self::Release { comment, .. } => *comment,
            Self::FeedbackRequest { body } => Some(*body),
        }
    }

    fn blocks_automation(&self) -> bool {
        match self {
            Self::Release {
                automation_disposition,
                ..
            } => *automation_disposition == ReleaseAutomationDisposition::Blocked,
            Self::FeedbackRequest { .. } => true,
        }
    }

    fn requests_feedback(&self) -> bool {
        matches!(self, Self::FeedbackRequest { .. })
    }

    fn update_context(&self) -> &'static str {
        match self {
            Self::Release { .. } => "failed to release work item",
            Self::FeedbackRequest { .. } => "failed to request item feedback",
        }
    }

    fn start_context(&self) -> &'static str {
        match self {
            Self::Release { .. } => "failed to start item release",
            Self::FeedbackRequest { .. } => "failed to start feedback request",
        }
    }

    fn event_type(&self) -> &'static str {
        match self {
            Self::Release { .. } => "item_released",
            Self::FeedbackRequest { .. } => "feedback_requested",
        }
    }

    fn event_body(&self, agent_id: &str, release_state: &str) -> String {
        match self {
            Self::Release { .. } => {
                format!("Released by {agent_id}; restored state to {release_state}")
            }
            Self::FeedbackRequest { .. } => {
                format!("Feedback requested by {agent_id}; restored state to {release_state}")
            }
        }
    }

    fn commit_context(&self) -> &'static str {
        match self {
            Self::Release { .. } => "failed to commit item release",
            Self::FeedbackRequest { .. } => "failed to commit feedback request",
        }
    }
}

#[derive(Debug)]
struct ActiveClaim {
    item: WorkItemModel,
}

impl ActiveClaim {
    fn touch_active_model(&self, updated_at: String) -> WorkItemActiveModel {
        let mut active: WorkItemActiveModel = self.item.clone().into();
        active.version = Set(self.item.version + 1);
        active.updated_at = Set(updated_at);
        active
    }

    fn clear_active_model(self, updated_at: String) -> WorkItemActiveModel {
        let version = self.item.version;
        let mut active: WorkItemActiveModel = self.item.into();
        active.claimed_by = Set(None);
        active.claimed_at = Set(None);
        active.claim_expires_at = Set(None);
        active.version = Set(version + 1);
        active.updated_at = Set(updated_at);
        active
    }
}

pub async fn has_claimable_item_matching_condition(
    store: &Store,
    project_name: &str,
    condition: &Condition,
) -> Result<bool> {
    let selector = item_labels::ValidatedLabelCondition::new(condition)?;
    let project_id = projects::project_id(store, project_name).await?;
    let items = claimable_items_in_claim_order(store.db().as_ref(), project_id).await?;
    let labels_by_item =
        labels_for_candidate_items(store.db().as_ref(), project_id, &items).await?;

    for item in items {
        let labels = labels_for_item(&labels_by_item, item.id);
        if selector.matches_automation_selector(labels) {
            return Ok(true);
        }
    }

    Ok(false)
}

pub async fn claim_item(
    store: &Store,
    project_name: &str,
    agent_id: &str,
    state_filter: &str,
) -> Result<Option<WorkItemView>> {
    validate_agent_id(agent_id)?;
    let state_filter = item_labels::normalize_state_value(state_filter)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item claim")?;
    let item = claim_first_matching_candidate_in_tx(&txn, project_id, agent_id, |labels| {
        !item_labels::is_automation_blocked(labels)
            && item_labels::current_state(labels).as_deref() == Some(state_filter.as_str())
    })
    .await?;

    commit_claim_transaction(
        store,
        project_name,
        txn,
        item,
        "failed to commit item claim",
    )
    .await
}

pub async fn claim_item_matching_condition(
    store: &Store,
    project_name: &str,
    agent_id: &str,
    condition: &Condition,
) -> Result<Option<WorkItemView>> {
    validate_agent_id(agent_id)?;
    let selector = item_labels::ValidatedLabelCondition::new(condition)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item claim")?;
    let item = claim_first_matching_candidate_in_tx(&txn, project_id, agent_id, |labels| {
        selector.matches_automation_selector(labels)
    })
    .await?;

    commit_claim_transaction(
        store,
        project_name,
        txn,
        item,
        "failed to commit item claim",
    )
    .await
}

pub async fn claim_specific_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
) -> Result<Option<WorkItemView>> {
    validate_agent_id(agent_id)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start specific item claim")?;
    let existing = work_items::get(&txn, project_id, item_id).await?;
    if existing.claimed_by.is_some() || existing.finished_at.is_some() {
        return commit_claim_transaction(
            store,
            project_name,
            txn,
            None,
            "failed to commit empty specific claim",
        )
        .await;
    }
    let labels = work_item_labels::for_item(&txn, project_id, item_id).await?;
    let source_state = item_labels::source_state_for_new_claim(&labels);
    let claimed = claim_candidate_in_tx(&txn, project_id, item_id, agent_id, &source_state).await?;

    commit_claim_transaction(
        store,
        project_name,
        txn,
        claimed,
        "failed to commit specific item claim",
    )
    .await
}

pub async fn release_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    comment: Option<String>,
    automation_disposition: ReleaseAutomationDisposition,
) -> Result<WorkItemView> {
    validate_agent_id(agent_id)?;
    return_claim_to_source_state(
        store,
        project_name,
        item_id,
        agent_id,
        ClaimReturnMode::Release {
            comment: comment.as_deref(),
            automation_disposition,
        },
    )
    .await
}

pub async fn request_feedback(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    body: &str,
) -> Result<WorkItemView> {
    validate_agent_id(agent_id)?;
    if body.trim().is_empty() {
        bail!("feedback request body cannot be empty");
    }

    return_claim_to_source_state(
        store,
        project_name,
        item_id,
        agent_id,
        ClaimReturnMode::FeedbackRequest { body },
    )
    .await
}

pub async fn progress_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    body: &str,
) -> Result<CommentView> {
    validate_agent_id(agent_id)?;
    if body.trim().is_empty() {
        bail!("progress body cannot be empty");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item progress")?;
    let claim = load_active_claim_in_tx(&txn, project_id, item_id, agent_id).await?;

    let comment = record_agent_comment_in_tx(&txn, item_id, agent_id, body).await?;

    let item_active = claim.touch_active_model(utc_now());
    item_active
        .update(&txn)
        .await
        .context("failed to update item after progress")?;
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "progress_added", body)
        .await?;
    txn.commit()
        .await
        .context("failed to commit item progress")?;
    events::publish_comment_changed(project_name, item_id);

    work_item_comments::to_view(comment)
}

pub async fn finish_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    report: &str,
) -> Result<WorkItemView> {
    validate_agent_id(agent_id)?;
    if report.trim().is_empty() {
        bail!("finish report cannot be empty");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item finish")?;
    let claim = load_active_claim_in_tx(&txn, project_id, item_id, agent_id).await?;

    let now = utc_now();
    record_agent_comment_in_tx(&txn, item_id, agent_id, report).await?;

    let mut active = claim.clear_active_model(now.clone());
    active.finished_at = Set(Some(now.clone()));
    let updated = active
        .update(&txn)
        .await
        .context("failed to finish work item")?;
    work_item_labels::upsert_in_tx(
        &txn,
        project_id,
        item_id,
        STATE_LABEL_KEY,
        Some(FINISHED_STATE_LABEL),
    )
    .await?;
    delete_workflow_labels_in_tx(
        &txn,
        project_id,
        item_id,
        &[
            CLAIMED_FROM_STATE_LABEL_KEY,
            AUTOMATION_BLOCKED_LABEL_KEY,
            FEEDBACK_REQUESTED_LABEL_KEY,
        ],
    )
    .await?;
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "item_finished", report)
        .await?;
    txn.commit().await.context("failed to commit item finish")?;
    events::publish_work_item_changed(project_name, item_id);

    work_items::model_to_view(store, updated).await
}

pub async fn recover_stale_claims(
    store: &Store,
    project_name: &str,
    stale_after_minutes: i64,
) -> Result<Vec<RecoveredClaimView>> {
    if stale_after_minutes <= 0 {
        return Ok(Vec::new());
    }

    let project_id = projects::project_id(store, project_name).await?;
    let items = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .filter(work_item::Column::ClaimedBy.is_not_null())
        .all(store.db().as_ref())
        .await
        .context("failed to list claimed work items")?;
    let cutoff = OffsetDateTime::now_utc() - Duration::minutes(stale_after_minutes);
    let mut recovered = Vec::new();

    for item in items {
        let Some(agent_id) = item.claimed_by.clone() else {
            continue;
        };
        let stale = match item.claim_expires_at.as_deref() {
            Some(expires_at) => timestamp_is_before_or_equal(expires_at, OffsetDateTime::now_utc()),
            None => item
                .claimed_at
                .as_deref()
                .map(|claimed_at| timestamp_is_before_or_equal(claimed_at, cutoff))
                .unwrap_or(false),
        };
        if !stale {
            continue;
        }

        let claim = RecoveredClaimView {
            item_id: item.id,
            agent_id: agent_id.clone(),
            claimed_at: item.claimed_at.clone(),
        };
        release_item(
            store,
            project_name,
            item.id,
            &agent_id,
            Some(format!(
                "Recovered stale claim after {stale_after_minutes} minute(s)."
            )),
            ReleaseAutomationDisposition::Claimable,
        )
        .await?;
        recovered.push(claim);
    }

    Ok(recovered)
}

async fn return_claim_to_source_state(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    mode: ClaimReturnMode<'_>,
) -> Result<WorkItemView> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store.db().begin().await.context(mode.start_context())?;
    let claim = load_active_claim_in_tx(&txn, project_id, item_id, agent_id).await?;
    let labels = work_item_labels::for_item(&txn, project_id, item_id).await?;
    let release_state = item_labels::release_state_from_claim_labels(&labels);

    let active = claim.clear_active_model(utc_now());
    let updated = active.update(&txn).await.context(mode.update_context())?;

    work_item_labels::upsert_in_tx(
        &txn,
        project_id,
        item_id,
        STATE_LABEL_KEY,
        Some(release_state.as_str()),
    )
    .await?;
    delete_workflow_labels_in_tx(&txn, project_id, item_id, &[CLAIMED_FROM_STATE_LABEL_KEY])
        .await?;
    if mode.blocks_automation() {
        work_item_labels::upsert_in_tx(
            &txn,
            project_id,
            item_id,
            AUTOMATION_BLOCKED_LABEL_KEY,
            None,
        )
        .await?;
    }
    if mode.requests_feedback() {
        work_item_labels::upsert_in_tx(
            &txn,
            project_id,
            item_id,
            FEEDBACK_REQUESTED_LABEL_KEY,
            None,
        )
        .await?;
    }

    if let Some(comment) = mode
        .agent_comment_body()
        .filter(|body| !body.trim().is_empty())
    {
        record_agent_comment_in_tx(&txn, item_id, agent_id, comment).await?;
    }

    let event_body = mode.event_body(agent_id, &release_state);
    work_item_events::record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        mode.event_type(),
        event_body.as_str(),
    )
    .await?;
    txn.commit().await.context(mode.commit_context())?;
    events::publish_work_item_changed(project_name, item_id);

    work_items::model_to_view(store, updated).await
}

async fn load_active_claim_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    agent_id: &str,
) -> Result<ActiveClaim>
where
    C: ConnectionTrait,
{
    let item = work_items::get(conn, project_id, item_id).await?;
    ensure_active_claim(&item, agent_id)?;
    Ok(ActiveClaim { item })
}

async fn record_agent_comment_in_tx<C>(
    conn: &C,
    item_id: i64,
    agent_id: &str,
    body: &str,
) -> Result<CommentModel>
where
    C: ConnectionTrait,
{
    work_item_comments::insert_in_tx(
        conn,
        item_id,
        AuthorType::Agent,
        Some(agent_id.to_owned()),
        body,
    )
    .await
}

async fn delete_workflow_labels_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    label_keys: &[&str],
) -> Result<()>
where
    C: ConnectionTrait,
{
    for label_key in label_keys {
        work_item_labels::delete_by_key_in_tx(conn, project_id, item_id, label_key).await?;
    }
    Ok(())
}

async fn claimable_items_in_claim_order<C>(conn: &C, project_id: i64) -> Result<Vec<WorkItemModel>>
where
    C: ConnectionTrait,
{
    Ok(WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .filter(work_item::Column::ClaimedBy.is_null())
        .filter(work_item::Column::FinishedAt.is_null())
        .order_by_asc(work_item::Column::UpdatedAt)
        .order_by_asc(work_item::Column::Id)
        .all(conn)
        .await
        .context("failed to list claimable work items")?)
}

async fn labels_for_candidate_items<C>(
    conn: &C,
    project_id: i64,
    items: &[WorkItemModel],
) -> Result<BTreeMap<i64, Vec<WorkItemLabelView>>>
where
    C: ConnectionTrait,
{
    if items.is_empty() {
        return Ok(BTreeMap::new());
    }

    let item_ids = items.iter().map(|item| item.id).collect::<Vec<_>>();
    work_item_labels::for_items(conn, project_id, &item_ids).await
}

fn labels_for_item(
    labels_by_item: &BTreeMap<i64, Vec<WorkItemLabelView>>,
    item_id: i64,
) -> &[WorkItemLabelView] {
    labels_by_item
        .get(&item_id)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

async fn claim_first_matching_candidate_in_tx<C, F>(
    conn: &C,
    project_id: i64,
    agent_id: &str,
    mut matches_candidate: F,
) -> Result<Option<WorkItemModel>>
where
    C: ConnectionTrait,
    F: FnMut(&[WorkItemLabelView]) -> bool,
{
    let candidates = claimable_items_in_claim_order(conn, project_id).await?;
    let labels_by_item = labels_for_candidate_items(conn, project_id, &candidates).await?;

    for candidate in candidates {
        let labels = labels_for_item(&labels_by_item, candidate.id);
        if !matches_candidate(labels) {
            continue;
        }

        let source_state = item_labels::source_state_for_new_claim(labels);
        let claimed =
            claim_candidate_in_tx(conn, project_id, candidate.id, agent_id, &source_state).await?;

        if claimed.is_some() {
            return Ok(claimed);
        }
    }

    Ok(None)
}

async fn claim_candidate_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    agent_id: &str,
    source_state: &str,
) -> Result<Option<WorkItemModel>>
where
    C: sea_orm::ConnectionTrait,
{
    let now = utc_now();
    let sql = r#"
        UPDATE work_items
        SET claimed_by = ?3,
            claimed_at = ?4,
            claim_expires_at = NULL,
            version = version + 1,
            updated_at = ?4
        WHERE id = ?2
          AND project_id = ?1
          AND claimed_by IS NULL
          AND finished_at IS NULL
        RETURNING id
        "#;

    let claimed_id = conn
        .query_one(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            sql,
            vec![
                project_id.into(),
                item_id.into(),
                agent_id.to_owned().into(),
                now.into(),
            ],
        ))
        .await
        .context("failed to claim work item")?
        .map(|row| row.try_get::<i64>("", "id"))
        .transpose()
        .context("failed to read claimed item id")?;

    let Some(item_id) = claimed_id else {
        return Ok(None);
    };

    Ok(Some(
        record_claim_in_tx(conn, project_id, item_id, agent_id, source_state).await?,
    ))
}

async fn commit_claim_transaction(
    store: &Store,
    project_name: &str,
    txn: DatabaseTransaction,
    item: Option<WorkItemModel>,
    commit_context: &'static str,
) -> Result<Option<WorkItemView>> {
    txn.commit().await.context(commit_context)?;

    let Some(item) = item else {
        return Ok(None);
    };

    events::publish_work_item_changed(project_name, item.id);
    Ok(Some(work_items::model_to_view(store, item).await?))
}

async fn record_claim_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    agent_id: &str,
    source_state: &str,
) -> Result<WorkItemModel>
where
    C: sea_orm::ConnectionTrait,
{
    work_item_labels::upsert_in_tx(
        conn,
        project_id,
        item_id,
        CLAIMED_FROM_STATE_LABEL_KEY,
        Some(source_state),
    )
    .await?;
    work_item_labels::upsert_in_tx(
        conn,
        project_id,
        item_id,
        STATE_LABEL_KEY,
        Some(CLAIMED_STATE_LABEL),
    )
    .await?;
    work_item_labels::delete_by_key_in_tx(conn, project_id, item_id, FEEDBACK_REQUESTED_LABEL_KEY)
        .await?;
    let comment_body = format!("Claimed by {agent_id}");
    work_item_comments::insert_in_tx(
        conn,
        item_id,
        AuthorType::System,
        None,
        comment_body.as_str(),
    )
    .await?;
    work_item_events::record_event_in_tx(
        conn,
        project_id,
        Some(item_id),
        "item_claimed",
        comment_body.as_str(),
    )
    .await?;
    work_items::get(conn, project_id, item_id).await
}

fn validate_agent_id(agent_id: &str) -> Result<()> {
    if agent_id.trim().is_empty() {
        bail!("agent id cannot be empty");
    }
    Ok(())
}

fn ensure_active_claim(item: &WorkItemModel, agent_id: &str) -> Result<()> {
    match item.claimed_by.as_deref() {
        Some(claimed_by) if claimed_by == agent_id => Ok(()),
        Some(claimed_by) => bail!("item {} is claimed by {claimed_by}", item.id),
        None => bail!("item {} is not claimed", item.id),
    }
}

fn timestamp_is_before_or_equal(value: &str, cutoff: OffsetDateTime) -> bool {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(|timestamp| timestamp <= cutoff)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use crudkit_core::condition::{
        Condition, ConditionClause, ConditionClauseValue, ConditionElement, Operator,
    };
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        agent_ids,
        comments::list_comments,
        entities::{agent_run, work_item::WorkItemActiveModel},
        item_label_service::add_label,
        items::{CreateWorkItem, create_item, get_item, list_events, list_items, move_item},
        projects::{self, CreateProject, create_project},
    };

    async fn test_store() -> (TempDir, Store) {
        let temp = TempDir::new().unwrap();
        let store = Store::open(temp.path().join("patchbay.sqlite3"))
            .await
            .unwrap();
        create_project(
            &store,
            CreateProject {
                name: "demo".to_owned(),
                display_name: None,
                path: temp.path().to_path_buf(),
                default_agent_model: None,
                default_agent_reasoning_effort: None,
                system_prompt: None,
                memory: None,
            },
        )
        .await
        .unwrap();
        create_project(
            &store,
            CreateProject {
                name: "other".to_owned(),
                display_name: None,
                path: temp.path().to_path_buf(),
                default_agent_model: None,
                default_agent_reasoning_effort: None,
                system_prompt: None,
                memory: None,
            },
        )
        .await
        .unwrap();
        (temp, store)
    }

    #[tokio::test]
    async fn claiming_item_records_agent_identity() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Claim me".to_owned(),
                description: "Available work".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let claimed = claim_item(&store, "demo", "agent-a", "open")
            .await
            .unwrap()
            .unwrap();
        let comments = list_comments(&store, "demo", item.id).await.unwrap();
        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(claimed.id, item.id);
        assert_eq!(claimed.state.as_deref(), Some("in_progress"));
        assert_eq!(claimed.version, item.version + 1);
        assert_eq!(claimed.claimed_by.as_deref(), Some("agent-a"));
        assert!(claimed.claimed_at.is_some());
        assert!(
            comments
                .iter()
                .any(|comment| comment.body == "Claimed by agent-a")
        );
        assert!(
            events
                .iter()
                .any(|event| event.event_type == "item_claimed")
        );
    }

    #[tokio::test]
    async fn claiming_can_use_nested_label_conditions() {
        let (_temp, store) = test_store().await;
        create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Plain item".to_owned(),
                description: "Should not match the selector".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let matching = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Urgent bug".to_owned(),
                description: "Should match the selector".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            matching.id,
            "severity".to_owned(),
            Some("high".to_owned()),
            None,
        )
        .await
        .unwrap();
        add_label(&store, "demo", matching.id, "bug".to_owned(), None, None)
            .await
            .unwrap();

        let selector = Condition::All(vec![
            ConditionElement::Clause(ConditionClause {
                column_name: "state".to_owned(),
                operator: Operator::Equal,
                value: ConditionClauseValue::String("open".to_owned()),
            }),
            ConditionElement::Condition(Box::new(Condition::Any(vec![
                ConditionElement::Clause(ConditionClause {
                    column_name: "severity".to_owned(),
                    operator: Operator::Equal,
                    value: ConditionClauseValue::String("high".to_owned()),
                }),
                ConditionElement::Clause(ConditionClause {
                    column_name: "bug".to_owned(),
                    operator: Operator::Equal,
                    value: ConditionClauseValue::Bool(true),
                }),
            ]))),
        ]);

        assert!(
            has_claimable_item_matching_condition(&store, "demo", &selector)
                .await
                .unwrap()
        );

        let claimed = claim_item_matching_condition(&store, "demo", "agent-a", &selector)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(claimed.id, matching.id);
        assert_eq!(claimed.claimed_by.as_deref(), Some("agent-a"));
        assert_eq!(claimed.state.as_deref(), Some("in_progress"));
    }

    #[tokio::test]
    async fn blocked_items_are_skipped_by_selector_claims() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Blocked bug".to_owned(),
                description: "Should wait for human triage".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            item.id,
            AUTOMATION_BLOCKED_LABEL_KEY.to_owned(),
            None,
            None,
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            item.id,
            "severity".to_owned(),
            Some("high".to_owned()),
            None,
        )
        .await
        .unwrap();

        let selector = Condition::All(vec![
            ConditionElement::Clause(ConditionClause {
                column_name: "state".to_owned(),
                operator: Operator::Equal,
                value: ConditionClauseValue::String("open".to_owned()),
            }),
            ConditionElement::Clause(ConditionClause {
                column_name: "severity".to_owned(),
                operator: Operator::Equal,
                value: ConditionClauseValue::String("high".to_owned()),
            }),
        ]);

        assert!(
            !has_claimable_item_matching_condition(&store, "demo", &selector)
                .await
                .unwrap()
        );
        let claimed = claim_item_matching_condition(&store, "demo", "agent-a", &selector)
            .await
            .unwrap();

        assert!(claimed.is_none());
    }

    #[tokio::test]
    async fn claiming_is_atomic_for_racing_agents() {
        let (_temp, store) = test_store().await;
        create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Race item".to_owned(),
                description: "Only one agent can own this".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let (first, second) = tokio::join!(
            claim_item(&store, "demo", "agent-a", "open"),
            claim_item(&store, "demo", "agent-b", "open")
        );
        let claims = [first.unwrap(), second.unwrap()];
        let in_progress = list_items(&store, "demo", Some("in_progress".to_owned()))
            .await
            .unwrap();

        assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
        assert_eq!(in_progress.len(), 1);
        assert!(matches!(
            in_progress[0].claimed_by.as_deref(),
            Some("agent-a" | "agent-b")
        ));
    }

    #[tokio::test]
    async fn claim_respects_project_scope() {
        let (_temp, store) = test_store().await;
        create_item(
            &store,
            "other",
            CreateWorkItem {
                title: "Other item".to_owned(),
                description: "Should not be claimed from demo".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let claimed = claim_item(&store, "demo", "agent-a", "open").await.unwrap();

        assert!(claimed.is_none());
    }

    #[tokio::test]
    async fn idea_item_is_skipped_until_moved_open() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Draft item".to_owned(),
                description: "Hold this back from automation".to_owned(),
                state: "idea".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let skipped = claim_item(&store, "demo", "agent-a", "open").await.unwrap();
        assert!(skipped.is_none());

        let opened = move_item(
            &store,
            "demo",
            item.id,
            "open".to_owned(),
            Some(item.version),
        )
        .await
        .unwrap();
        let claimed = claim_item(&store, "demo", "agent-a", "open")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(opened.state.as_deref(), Some("open"));
        assert_eq!(claimed.id, item.id);
        assert_eq!(claimed.claimed_by.as_deref(), Some("agent-a"));
    }

    #[tokio::test]
    async fn claimed_items_include_verified_automation_source() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Refine me".to_owned(),
                description: "A trigger should be visible while claimed".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let now = utc_now();
        let run = agent_run::ActiveModel {
            project_id: Set(item.project_id),
            work_item_id: Set(Some(item.id)),
            memory_event_id: Set(None),
            trigger_id: Set(Some(7)),
            trigger_name: Set(Some("Refine queued item".to_owned())),
            tool_name: Set("codex".to_owned()),
            mutability: Set("read_only".to_owned()),
            status: Set("running".to_owned()),
            command: Set(String::new()),
            working_dir: Set(String::new()),
            worktree_path: Set(None),
            branch_name: Set(None),
            process_id: Set(None),
            exit_code: Set(None),
            log_path: Set(None),
            prompt_path: Set(None),
            agent_model: Set(None),
            agent_reasoning_effort: Set(None),
            input_tokens: Set(None),
            cached_input_tokens: Set(None),
            output_tokens: Set(None),
            commit_required: Set(false),
            commit_outcome: Set("not_evaluated".to_owned()),
            commit_shas: Set("[]".to_owned()),
            pr_requested: Set(false),
            pr_url: Set(None),
            cleanup_status: Set("not_applicable".to_owned()),
            worktree_cleaned_at: Set(None),
            result_summary: Set(String::new()),
            started_at: Set(Some(now.clone())),
            finished_at: Set(None),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(store.db().as_ref())
        .await
        .unwrap();
        let agent_id = agent_ids::patchbay_run_agent_id(run.id);

        claim_specific_item(&store, "demo", item.id, &agent_id)
            .await
            .unwrap()
            .unwrap();
        let item = get_item(&store, "demo", item.id).await.unwrap();
        let listed = list_items(&store, "demo", None)
            .await
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == item.id)
            .unwrap();

        for view in [item, listed] {
            let claim_source = view.claim_source.expect("claim source should be present");
            assert_eq!(claim_source.run_id, run.id);
            assert_eq!(claim_source.trigger_id, Some(7));
            assert_eq!(
                claim_source.trigger_name.as_deref(),
                Some("Refine queued item")
            );
        }
    }

    #[tokio::test]
    async fn claimed_items_ignore_unlinked_patchbay_run_claimants() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Claim me".to_owned(),
                description: "Source should not be guessed from a mismatched run".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let other = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Other".to_owned(),
                description: "The run is structurally linked here instead".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let now = utc_now();
        let run = agent_run::ActiveModel {
            project_id: Set(item.project_id),
            work_item_id: Set(Some(other.id)),
            memory_event_id: Set(None),
            trigger_id: Set(Some(8)),
            trigger_name: Set(Some("Wrong source".to_owned())),
            tool_name: Set("codex".to_owned()),
            mutability: Set("mutating".to_owned()),
            status: Set("running".to_owned()),
            command: Set(String::new()),
            working_dir: Set(String::new()),
            worktree_path: Set(None),
            branch_name: Set(None),
            process_id: Set(None),
            exit_code: Set(None),
            log_path: Set(None),
            prompt_path: Set(None),
            agent_model: Set(None),
            agent_reasoning_effort: Set(None),
            input_tokens: Set(None),
            cached_input_tokens: Set(None),
            output_tokens: Set(None),
            commit_required: Set(false),
            commit_outcome: Set("not_evaluated".to_owned()),
            commit_shas: Set("[]".to_owned()),
            pr_requested: Set(false),
            pr_url: Set(None),
            cleanup_status: Set("not_applicable".to_owned()),
            worktree_cleaned_at: Set(None),
            result_summary: Set(String::new()),
            started_at: Set(Some(now.clone())),
            finished_at: Set(None),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(store.db().as_ref())
        .await
        .unwrap();
        let agent_id = agent_ids::patchbay_run_agent_id(run.id);

        claim_specific_item(&store, "demo", item.id, &agent_id)
            .await
            .unwrap()
            .unwrap();
        let item = get_item(&store, "demo", item.id).await.unwrap();

        assert_eq!(item.claimed_by.as_deref(), Some(agent_id.as_str()));
        assert!(item.claim_source.is_none());
    }

    #[tokio::test]
    async fn release_restores_claim_source_state_and_blocks_automation() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Custom lane item".to_owned(),
                description: "Release should return to this lane".to_owned(),
                state: "ready".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let claimed = claim_item(&store, "demo", "agent-a", "ready")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.state.as_deref(), Some(CLAIMED_STATE_LABEL));
        assert!(claimed.labels.iter().any(|label| {
            label.key == CLAIMED_FROM_STATE_LABEL_KEY && label.value.as_deref() == Some("ready")
        }));

        let released = release_item(
            &store,
            "demo",
            item.id,
            "agent-a",
            Some("Cannot operate on this item.".to_owned()),
            ReleaseAutomationDisposition::Blocked,
        )
        .await
        .unwrap();

        assert_eq!(released.state.as_deref(), Some("ready"));
        assert_eq!(released.claimed_by, None);
        assert!(
            released
                .labels
                .iter()
                .any(|label| label.key == AUTOMATION_BLOCKED_LABEL_KEY)
        );
        assert!(
            released
                .labels
                .iter()
                .all(|label| label.key != CLAIMED_FROM_STATE_LABEL_KEY)
        );

        let claimed_again = claim_item(&store, "demo", "agent-b", "ready")
            .await
            .unwrap();
        assert!(claimed_again.is_none());
    }

    #[tokio::test]
    async fn request_feedback_restores_source_state_and_blocks_automation() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Needs input".to_owned(),
                description: "Agent should ask for a user decision".to_owned(),
                state: "ready".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", "ready")
            .await
            .unwrap()
            .unwrap();

        let updated = request_feedback(
            &store,
            "demo",
            item.id,
            "agent-a",
            "Which provider should this integration target?",
        )
        .await
        .unwrap();
        let comments = list_comments(&store, "demo", item.id).await.unwrap();
        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(updated.state.as_deref(), Some("ready"));
        assert_eq!(updated.claimed_by, None);
        assert!(
            updated
                .labels
                .iter()
                .any(|label| label.key == AUTOMATION_BLOCKED_LABEL_KEY)
        );
        assert!(
            updated
                .labels
                .iter()
                .any(|label| label.key == FEEDBACK_REQUESTED_LABEL_KEY)
        );
        assert!(
            updated
                .labels
                .iter()
                .all(|label| label.key != CLAIMED_FROM_STATE_LABEL_KEY)
        );
        assert!(comments.iter().any(|comment| {
            comment.author_type == AuthorType::Agent
                && comment.author_name.as_deref() == Some("agent-a")
                && comment.body == "Which provider should this integration target?"
        }));
        assert!(
            events
                .iter()
                .any(|event| event.event_type == "feedback_requested")
        );

        let claimed_again = claim_item(&store, "demo", "agent-b", "ready")
            .await
            .unwrap();
        assert!(claimed_again.is_none());
    }

    #[tokio::test]
    async fn feedback_requested_label_blocks_state_claims() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Awaiting answer".to_owned(),
                description: "Feedback label alone should block automation pickup".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            item.id,
            FEEDBACK_REQUESTED_LABEL_KEY.to_owned(),
            None,
            None,
        )
        .await
        .unwrap();

        let claimed = claim_item(&store, "demo", "agent-a", "open").await.unwrap();

        assert!(claimed.is_none());
    }

    #[tokio::test]
    async fn specific_claim_release_restores_current_state() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Manual retry".to_owned(),
                description: "Explicit item claims are not tied to open".to_owned(),
                state: "triage".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let claimed = claim_specific_item(&store, "demo", item.id, "agent-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.state.as_deref(), Some(CLAIMED_STATE_LABEL));

        let released = release_item(
            &store,
            "demo",
            item.id,
            "agent-a",
            None,
            ReleaseAutomationDisposition::Claimable,
        )
        .await
        .unwrap();

        assert_eq!(released.state.as_deref(), Some("triage"));
        assert_eq!(released.claimed_by, None);
        assert!(
            released
                .labels
                .iter()
                .all(|label| label.key != AUTOMATION_BLOCKED_LABEL_KEY)
        );

        let claimed_again = claim_item(&store, "demo", "agent-b", "triage")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed_again.id, item.id);
        assert_eq!(claimed_again.claimed_by.as_deref(), Some("agent-b"));
    }

    #[tokio::test]
    async fn new_claims_overwrite_stale_claim_source_with_current_state() {
        let (_temp, store) = test_store().await;
        let state_item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "State retry".to_owned(),
                description: "State claims use the current state as release source".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            state_item.id,
            CLAIMED_FROM_STATE_LABEL_KEY.to_owned(),
            Some("ready".to_owned()),
            None,
        )
        .await
        .unwrap();

        let claimed = claim_item(&store, "demo", "agent-state", "open")
            .await
            .unwrap()
            .unwrap();

        assert!(claimed.labels.iter().any(|label| {
            label.key == CLAIMED_FROM_STATE_LABEL_KEY && label.value.as_deref() == Some("open")
        }));

        let released = release_item(
            &store,
            "demo",
            state_item.id,
            "agent-state",
            None,
            ReleaseAutomationDisposition::Claimable,
        )
        .await
        .unwrap();

        assert_eq!(released.state.as_deref(), Some("open"));

        let selector_item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Selector retry".to_owned(),
                description: "Claim source should come from the current state label".to_owned(),
                state: "ready".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            selector_item.id,
            CLAIMED_FROM_STATE_LABEL_KEY.to_owned(),
            Some("open".to_owned()),
            None,
        )
        .await
        .unwrap();

        let selector = Condition::All(vec![ConditionElement::Clause(ConditionClause {
            column_name: STATE_LABEL_KEY.to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::String("ready".to_owned()),
        })]);
        let claimed = claim_item_matching_condition(&store, "demo", "agent-a", &selector)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(claimed.state.as_deref(), Some(CLAIMED_STATE_LABEL));
        assert!(claimed.labels.iter().any(|label| {
            label.key == CLAIMED_FROM_STATE_LABEL_KEY && label.value.as_deref() == Some("ready")
        }));

        let released = release_item(
            &store,
            "demo",
            selector_item.id,
            "agent-a",
            None,
            ReleaseAutomationDisposition::Claimable,
        )
        .await
        .unwrap();

        assert_eq!(released.state.as_deref(), Some("ready"));

        let specific_item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Specific retry".to_owned(),
                description: "Specific claims use the same source-state rule".to_owned(),
                state: "triage".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            specific_item.id,
            CLAIMED_FROM_STATE_LABEL_KEY.to_owned(),
            Some("open".to_owned()),
            None,
        )
        .await
        .unwrap();

        let claimed = claim_specific_item(&store, "demo", specific_item.id, "agent-b")
            .await
            .unwrap()
            .unwrap();

        assert!(claimed.labels.iter().any(|label| {
            label.key == CLAIMED_FROM_STATE_LABEL_KEY && label.value.as_deref() == Some("triage")
        }));

        let released = release_item(
            &store,
            "demo",
            specific_item.id,
            "agent-b",
            None,
            ReleaseAutomationDisposition::Claimable,
        )
        .await
        .unwrap();

        assert_eq!(released.state.as_deref(), Some("triage"));
    }

    #[tokio::test]
    async fn release_requires_current_claimant() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Owned item".to_owned(),
                description: "Only the claimant can release it".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", "open")
            .await
            .unwrap()
            .unwrap();

        let err = release_item(
            &store,
            "demo",
            item.id,
            "agent-b",
            None,
            ReleaseAutomationDisposition::Blocked,
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("claimed by agent-a"));
    }

    #[tokio::test]
    async fn finish_clears_claim_and_blocking_workflow_labels() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Finish blocked item".to_owned(),
                description: "Completion should clear workflow bookkeeping labels".to_owned(),
                state: "ready".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            item.id,
            AUTOMATION_BLOCKED_LABEL_KEY.to_owned(),
            None,
            None,
        )
        .await
        .unwrap();
        claim_specific_item(&store, "demo", item.id, "agent-a")
            .await
            .unwrap()
            .unwrap();
        add_label(
            &store,
            "demo",
            item.id,
            FEEDBACK_REQUESTED_LABEL_KEY.to_owned(),
            None,
            None,
        )
        .await
        .unwrap();

        let finished = finish_item(&store, "demo", item.id, "agent-a", "Finished cleanly")
            .await
            .unwrap();

        assert_eq!(finished.state.as_deref(), Some(FINISHED_STATE_LABEL));
        assert_eq!(finished.claimed_by, None);
        assert!(finished.finished_at.is_some());
        for key in [
            CLAIMED_FROM_STATE_LABEL_KEY,
            AUTOMATION_BLOCKED_LABEL_KEY,
            FEEDBACK_REQUESTED_LABEL_KEY,
        ] {
            assert!(
                finished.labels.iter().all(|label| label.key != key),
                "finished item should not retain {key}"
            );
        }
    }

    #[tokio::test]
    async fn finish_moves_done_and_records_report() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Finish item".to_owned(),
                description: "Complete with report".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", "open")
            .await
            .unwrap()
            .unwrap();

        let finished = finish_item(&store, "demo", item.id, "agent-a", "Finished cleanly")
            .await
            .unwrap();
        let comments = list_comments(&store, "demo", item.id).await.unwrap();
        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(finished.state.as_deref(), Some("done"));
        assert_eq!(finished.claimed_by, None);
        assert!(finished.finished_at.is_some());
        assert!(
            comments
                .iter()
                .any(|comment| comment.body == "Finished cleanly")
        );
        assert!(
            events
                .iter()
                .any(|event| event.event_type == "item_finished")
        );
    }

    #[tokio::test]
    async fn state_and_selector_claims_do_not_reopen_finished_items() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Finished item".to_owned(),
                description: "State changes alone should not reopen finished work".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", "open")
            .await
            .unwrap()
            .unwrap();
        let finished = finish_item(&store, "demo", item.id, "agent-a", "Finished")
            .await
            .unwrap();
        let moved = move_item(
            &store,
            "demo",
            item.id,
            "open".to_owned(),
            Some(finished.version),
        )
        .await
        .unwrap();
        let selector = Condition::All(vec![ConditionElement::Clause(ConditionClause {
            column_name: STATE_LABEL_KEY.to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::String("open".to_owned()),
        })]);

        let state_claim = claim_item(&store, "demo", "agent-b", "open").await.unwrap();
        let selector_has_match = has_claimable_item_matching_condition(&store, "demo", &selector)
            .await
            .unwrap();
        let selector_claim = claim_item_matching_condition(&store, "demo", "agent-c", &selector)
            .await
            .unwrap();
        let reloaded = get_item(&store, "demo", item.id).await.unwrap();

        assert_eq!(moved.state.as_deref(), Some("open"));
        assert!(moved.finished_at.is_some());
        assert!(state_claim.is_none());
        assert!(!selector_has_match);
        assert!(selector_claim.is_none());
        assert_eq!(reloaded.claimed_by, None);
        assert!(reloaded.finished_at.is_some());
    }

    #[tokio::test]
    async fn specific_claim_does_not_reopen_finished_items() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Finished item".to_owned(),
                description: "Should stay closed after completion".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", "open")
            .await
            .unwrap()
            .unwrap();
        finish_item(&store, "demo", item.id, "agent-a", "Finished")
            .await
            .unwrap();

        let claimed = claim_specific_item(&store, "demo", item.id, "agent-b")
            .await
            .unwrap();
        let reloaded = get_item(&store, "demo", item.id).await.unwrap();

        assert!(claimed.is_none());
        assert_eq!(reloaded.state.as_deref(), Some(FINISHED_STATE_LABEL));
        assert_eq!(reloaded.claimed_by, None);
        assert!(reloaded.finished_at.is_some());
    }

    #[tokio::test]
    async fn stale_claim_recovery_releases_old_claim() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Stale item".to_owned(),
                description: "Claim should be recovered".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", "open")
            .await
            .unwrap()
            .unwrap();
        let project_id = projects::project_id(&store, "demo").await.unwrap();
        let mut model: WorkItemActiveModel =
            work_items::get(store.db().as_ref(), project_id, item.id)
                .await
                .unwrap()
                .into();
        model.claimed_at = Set(Some(
            (OffsetDateTime::now_utc() - Duration::minutes(30))
                .format(&Rfc3339)
                .unwrap(),
        ));
        model.update(store.db().as_ref()).await.unwrap();

        let recovered = recover_stale_claims(&store, "demo", 10).await.unwrap();
        let item = get_item(&store, "demo", item.id).await.unwrap();

        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].agent_id, "agent-a");
        assert_eq!(item.state.as_deref(), Some("open"));
        assert_eq!(item.claimed_by, None);
    }
}
