use std::{collections::BTreeMap, str::FromStr};

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
            agent_run::{self, AgentRun, AgentRunModel},
            comment::{CommentActiveModel, CommentModel},
            work_item::{self, WorkItem, WorkItemActiveModel, WorkItemModel},
            work_item_event,
            work_item_label::{self, WorkItemLabel, WorkItemLabelActiveModel, WorkItemLabelModel},
        },
        events, item_labels, projects,
        storage::{Store, utc_now},
        work_item_events,
    },
    shared::view_models::{
        AUTOMATION_BLOCKED_LABEL_KEY, AgentReasoningEffort, AuthorType,
        CLAIMED_FROM_STATE_LABEL_KEY, CLAIMED_STATE_LABEL, CommentView,
        DeleteWorkItemLabelResponse, FEEDBACK_REQUESTED_LABEL_KEY, FINISHED_STATE_LABEL,
        ProjectLabelView, RecoveredClaimView, STATE_LABEL_KEY, WorkItemClaimSourceView,
        WorkItemEventView, WorkItemLabelView, WorkItemView,
    },
};

#[derive(Clone, Debug)]
pub struct CreateWorkItem {
    pub title: String,
    pub description: String,
    pub state: String,
    pub agent_model_override: Option<String>,
    pub agent_reasoning_effort_override: Option<AgentReasoningEffort>,
}

#[derive(Clone, Debug, Default)]
pub struct UpdateWorkItem {
    pub title: Option<String>,
    pub description: Option<String>,
    pub state: Option<String>,
    pub agent_model_override: Option<Option<String>>,
    pub agent_reasoning_effort_override: Option<Option<AgentReasoningEffort>>,
    pub expect_version: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseAutomationDisposition {
    Claimable,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FinishedClaimPolicy {
    AllowFinished,
    RejectFinished,
}

pub async fn list_items(
    store: &Store,
    project_name: &str,
    state: Option<String>,
) -> Result<Vec<WorkItemView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let item_ids = match state {
        Some(state) => {
            let state = item_labels::normalize_state_value(state)?;
            let ids = item_ids_with_state(store.db().as_ref(), project_id, &state).await?;
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            Some(ids)
        }
        None => None,
    };
    let mut query = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .order_by_desc(work_item::Column::UpdatedAt)
        .order_by_desc(work_item::Column::Id);

    if let Some(item_ids) = item_ids {
        query = query.filter(work_item::Column::Id.is_in(item_ids));
    }

    let items = query
        .all(store.db().as_ref())
        .await
        .context("failed to list work items")?;
    models_to_views(store, project_id, items).await
}

pub async fn count_items_outside_work_item_states(
    store: &Store,
    project_name: &str,
) -> Result<i64> {
    let project_id = projects::project_id(store, project_name).await?;
    let row = store
        .db()
        .query_one(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r#"
            SELECT COUNT(*) AS count
            FROM work_items AS wi
            WHERE wi.project_id = ?1
              AND NOT EXISTS (
                  SELECT 1
                  FROM work_item_labels AS wil
                  JOIN work_item_states AS wis
                    ON wis.project_id = wi.project_id
                   AND wis.identifier = wil.label_value
                  WHERE wil.project_id = wi.project_id
                    AND wil.work_item_id = wi.id
                    AND wil.label_key = ?2
              )
            "#,
            vec![project_id.into(), STATE_LABEL_KEY.to_owned().into()],
        ))
        .await
        .context("failed to count work items outside authored states")?;

    row.map(|row| row.try_get::<i64>("", "count"))
        .transpose()
        .context("failed to read work items outside authored states count")?
        .ok_or_else(|| report!("missing work items outside authored states count"))
}

pub async fn has_unclaimed_item_matching_condition(
    store: &Store,
    project_name: &str,
    condition: &Condition,
) -> Result<bool> {
    item_labels::validate_condition(condition)?;
    let project_id = projects::project_id(store, project_name).await?;
    let items = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .filter(work_item::Column::ClaimedBy.is_null())
        .order_by_asc(work_item::Column::UpdatedAt)
        .order_by_asc(work_item::Column::Id)
        .all(store.db().as_ref())
        .await
        .context("failed to list unclaimed work items")?;

    for item in items {
        let labels = labels_for_item(store.db().as_ref(), project_id, item.id).await?;
        if item_labels::is_automation_blocked(&labels) {
            continue;
        }
        if item_labels::condition_matches(condition, &labels)? {
            return Ok(true);
        }
    }

    Ok(false)
}

pub async fn item_matches_condition(
    store: &Store,
    project_name: &str,
    item_id: i64,
    condition: &Condition,
) -> Result<bool> {
    item_labels::validate_condition(condition)?;
    let project_id = projects::project_id(store, project_name).await?;
    let item = get_item_model(store, project_id, item_id).await?;
    let labels = labels_for_item(store.db().as_ref(), project_id, item.id).await?;
    if item_labels::is_automation_blocked(&labels) {
        return Ok(false);
    }
    item_labels::condition_matches(condition, &labels)
}

pub async fn get_item(store: &Store, project_name: &str, item_id: i64) -> Result<WorkItemView> {
    let project_id = projects::project_id(store, project_name).await?;
    let item = get_item_model(store, project_id, item_id).await?;
    model_to_view(store, item).await
}

pub async fn create_item(
    store: &Store,
    project_name: &str,
    create: CreateWorkItem,
) -> Result<WorkItemView> {
    validate_item_text(&create.title, &create.description)?;
    let state_label = item_labels::normalize_state_value(create.state)?;
    let agent_model_override = projects::normalize_optional(create.agent_model_override);

    let project_id = projects::project_id(store, project_name).await?;
    let now = utc_now();
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item create")?;

    let active = WorkItemActiveModel {
        project_id: Set(project_id),
        title: Set(create.title),
        description: Set(create.description),
        agent_model_override: Set(agent_model_override),
        agent_reasoning_effort_override: Set(create
            .agent_reasoning_effort_override
            .map(|effort| effort.as_storage().to_owned())),
        version: Set(1),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };
    let item = active
        .insert(&txn)
        .await
        .context("failed to create work item")?;
    upsert_label_in_tx(
        &txn,
        project_id,
        item.id,
        STATE_LABEL_KEY,
        Some(state_label.as_str()),
    )
    .await?;
    work_item_events::record_event_in_tx(
        &txn,
        project_id,
        Some(item.id),
        "item_created",
        "Created item",
    )
    .await?;
    txn.commit().await.context("failed to commit item create")?;
    events::publish_work_item_changed(project_name, item.id);

    model_to_view(store, item).await
}

pub async fn update_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    update: UpdateWorkItem,
) -> Result<WorkItemView> {
    let state = update
        .state
        .map(item_labels::normalize_state_value)
        .transpose()
        .context("invalid item state")?;
    let has_item_update = update.title.is_some()
        || update.description.is_some()
        || update.agent_model_override.is_some()
        || update.agent_reasoning_effort_override.is_some();
    let has_text_update = update.title.is_some() || update.description.is_some();
    if update.title.is_none()
        && update.description.is_none()
        && state.is_none()
        && update.agent_model_override.is_none()
        && update.agent_reasoning_effort_override.is_none()
    {
        bail!("item update requires at least one field");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item update")?;
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
    check_expected_version(update.expect_version, existing.version)?;

    let title = update.title.unwrap_or_else(|| existing.title.clone());
    let description = update
        .description
        .unwrap_or_else(|| existing.description.clone());
    if has_text_update {
        validate_item_text(&title, &description)?;
    }

    let version = existing.version;
    let mut active: WorkItemActiveModel = existing.into();
    active.title = Set(title);
    active.description = Set(description);
    if let Some(agent_model_override) = update.agent_model_override {
        active.agent_model_override = Set(projects::normalize_optional(agent_model_override));
    }
    if let Some(agent_reasoning_effort_override) = update.agent_reasoning_effort_override {
        active.agent_reasoning_effort_override =
            Set(agent_reasoning_effort_override.map(|effort| effort.as_storage().to_owned()));
    }
    active.version = Set(version + 1);
    active.updated_at = Set(utc_now());

    let updated = active
        .update(&txn)
        .await
        .context("failed to update work item")?;
    if has_item_update {
        work_item_events::record_event_in_tx(
            &txn,
            project_id,
            Some(item_id),
            "item_updated",
            "Updated item",
        )
        .await?;
    }
    if let Some(state) = state {
        upsert_label_in_tx(
            &txn,
            project_id,
            item_id,
            STATE_LABEL_KEY,
            Some(state.as_str()),
        )
        .await?;
        let event_body = format!("Moved item to {state}");
        work_item_events::record_event_in_tx(
            &txn,
            project_id,
            Some(item_id),
            "item_moved",
            &event_body,
        )
        .await?;
    }
    txn.commit().await.context("failed to commit item update")?;
    events::publish_work_item_changed(project_name, item_id);

    model_to_view(store, updated).await
}

pub async fn move_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    state: String,
    expect_version: Option<i64>,
) -> Result<WorkItemView> {
    update_item(
        store,
        project_name,
        item_id,
        UpdateWorkItem {
            title: None,
            description: None,
            state: Some(state),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            expect_version,
        },
    )
    .await
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
    let now = utc_now();

    let claimed_id = txn
        .query_one(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r#"
            UPDATE work_items
            SET claimed_by = ?2,
                claimed_at = ?3,
                claim_expires_at = NULL,
                finished_at = NULL,
                version = version + 1,
                updated_at = ?3
            WHERE id = (
                SELECT work_items.id
                FROM work_items
                INNER JOIN work_item_labels
                    ON work_item_labels.work_item_id = work_items.id
                   AND work_item_labels.label_key = ?5
                   AND work_item_labels.label_value = ?4
                WHERE work_items.project_id = ?1
                  AND claimed_by IS NULL
                  AND NOT EXISTS (
                      SELECT 1
                      FROM work_item_labels blocked_labels
                      WHERE blocked_labels.work_item_id = work_items.id
                        AND blocked_labels.label_key IN (?6, ?7)
                  )
                ORDER BY work_items.updated_at ASC, work_items.id ASC
                LIMIT 1
            )
            RETURNING id
            "#,
            vec![
                project_id.into(),
                agent_id.to_owned().into(),
                now.clone().into(),
                state_filter.clone().into(),
                STATE_LABEL_KEY.into(),
                AUTOMATION_BLOCKED_LABEL_KEY.into(),
                FEEDBACK_REQUESTED_LABEL_KEY.into(),
            ],
        ))
        .await
        .context("failed to claim work item")?
        .map(|row| row.try_get::<i64>("", "id"))
        .transpose()
        .context("failed to read claimed item id")?;

    let item =
        record_claimed_id_in_tx(&txn, project_id, claimed_id, agent_id, &state_filter).await?;
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
    item_labels::validate_condition(condition)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item claim")?;
    let candidates = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .filter(work_item::Column::ClaimedBy.is_null())
        .order_by_asc(work_item::Column::UpdatedAt)
        .order_by_asc(work_item::Column::Id)
        .all(&txn)
        .await
        .context("failed to list claimable work items")?;

    for candidate in candidates {
        let labels = labels_for_item(&txn, project_id, candidate.id).await?;
        if item_labels::is_automation_blocked(&labels) {
            continue;
        }
        if !item_labels::condition_matches(condition, &labels)? {
            continue;
        }
        let source_state = item_labels::source_state_for_new_claim(&labels);

        let claimed = claim_candidate_in_tx(
            &txn,
            project_id,
            candidate.id,
            agent_id,
            &source_state,
            FinishedClaimPolicy::AllowFinished,
        )
        .await?;

        let Some(item) = claimed else {
            continue;
        };

        return commit_claim_transaction(
            store,
            project_name,
            txn,
            Some(item),
            "failed to commit item claim",
        )
        .await;
    }

    commit_claim_transaction(
        store,
        project_name,
        txn,
        None,
        "failed to commit empty claim",
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
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
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
    let labels = labels_for_item(&txn, project_id, item_id).await?;
    let source_state = item_labels::source_state_for_new_claim(&labels);
    let claimed = claim_candidate_in_tx(
        &txn,
        project_id,
        item_id,
        agent_id,
        &source_state,
        FinishedClaimPolicy::RejectFinished,
    )
    .await?;

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

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item release")?;
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
    ensure_active_claim(&existing, agent_id)?;
    let labels = labels_for_item(&txn, project_id, item_id).await?;
    let release_state = item_labels::release_state_from_claim_labels(&labels);

    let now = utc_now();
    let version = existing.version;
    let mut active: WorkItemActiveModel = existing.into();
    active.claimed_by = Set(None);
    active.claimed_at = Set(None);
    active.claim_expires_at = Set(None);
    active.version = Set(version + 1);
    active.updated_at = Set(now);
    let updated = active
        .update(&txn)
        .await
        .context("failed to release work item")?;
    upsert_label_in_tx(
        &txn,
        project_id,
        item_id,
        STATE_LABEL_KEY,
        Some(release_state.as_str()),
    )
    .await?;
    delete_label_by_key_in_tx(&txn, project_id, item_id, CLAIMED_FROM_STATE_LABEL_KEY).await?;
    if automation_disposition == ReleaseAutomationDisposition::Blocked {
        upsert_label_in_tx(
            &txn,
            project_id,
            item_id,
            AUTOMATION_BLOCKED_LABEL_KEY,
            None,
        )
        .await?;
    }

    if let Some(comment) = comment.filter(|body| !body.trim().is_empty()) {
        insert_comment_in_tx(
            &txn,
            item_id,
            AuthorType::Agent,
            Some(agent_id.to_owned()),
            comment.as_str(),
        )
        .await?;
    }

    let event_body = format!("Released by {agent_id}; restored state to {release_state}");
    work_item_events::record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "item_released",
        event_body.as_str(),
    )
    .await?;
    txn.commit()
        .await
        .context("failed to commit item release")?;
    events::publish_work_item_changed(project_name, item_id);

    model_to_view(store, updated).await
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

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start feedback request")?;
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
    ensure_active_claim(&existing, agent_id)?;
    let labels = labels_for_item(&txn, project_id, item_id).await?;
    let release_state = item_labels::release_state_from_claim_labels(&labels);

    let now = utc_now();
    insert_comment_in_tx(
        &txn,
        item_id,
        AuthorType::Agent,
        Some(agent_id.to_owned()),
        body,
    )
    .await?;

    let version = existing.version;
    let mut active: WorkItemActiveModel = existing.into();
    active.claimed_by = Set(None);
    active.claimed_at = Set(None);
    active.claim_expires_at = Set(None);
    active.version = Set(version + 1);
    active.updated_at = Set(now);
    let updated = active
        .update(&txn)
        .await
        .context("failed to request item feedback")?;
    upsert_label_in_tx(
        &txn,
        project_id,
        item_id,
        STATE_LABEL_KEY,
        Some(release_state.as_str()),
    )
    .await?;
    delete_label_by_key_in_tx(&txn, project_id, item_id, CLAIMED_FROM_STATE_LABEL_KEY).await?;
    upsert_label_in_tx(
        &txn,
        project_id,
        item_id,
        AUTOMATION_BLOCKED_LABEL_KEY,
        None,
    )
    .await?;
    upsert_label_in_tx(
        &txn,
        project_id,
        item_id,
        FEEDBACK_REQUESTED_LABEL_KEY,
        None,
    )
    .await?;

    let event_body = format!("Feedback requested by {agent_id}; restored state to {release_state}");
    work_item_events::record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "feedback_requested",
        &event_body,
    )
    .await?;
    txn.commit()
        .await
        .context("failed to commit feedback request")?;
    events::publish_work_item_changed(project_name, item_id);

    model_to_view(store, updated).await
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
    let item = get_item_model_in_tx(&txn, project_id, item_id).await?;
    ensure_active_claim(&item, agent_id)?;

    let comment = insert_comment_in_tx(
        &txn,
        item_id,
        AuthorType::Agent,
        Some(agent_id.to_owned()),
        body,
    )
    .await?;

    let mut item_active: WorkItemActiveModel = item.clone().into();
    item_active.version = Set(item.version + 1);
    item_active.updated_at = Set(utc_now());
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

    comment_to_view(comment)
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
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
    ensure_active_claim(&existing, agent_id)?;

    let now = utc_now();
    insert_comment_in_tx(
        &txn,
        item_id,
        AuthorType::Agent,
        Some(agent_id.to_owned()),
        report,
    )
    .await?;

    let version = existing.version;
    let mut active: WorkItemActiveModel = existing.into();
    active.claimed_by = Set(None);
    active.claimed_at = Set(None);
    active.claim_expires_at = Set(None);
    active.finished_at = Set(Some(now.clone()));
    active.version = Set(version + 1);
    active.updated_at = Set(now);
    let updated = active
        .update(&txn)
        .await
        .context("failed to finish work item")?;
    upsert_label_in_tx(
        &txn,
        project_id,
        item_id,
        STATE_LABEL_KEY,
        Some(FINISHED_STATE_LABEL),
    )
    .await?;
    delete_label_by_key_in_tx(&txn, project_id, item_id, CLAIMED_FROM_STATE_LABEL_KEY).await?;
    delete_label_by_key_in_tx(&txn, project_id, item_id, AUTOMATION_BLOCKED_LABEL_KEY).await?;
    delete_label_by_key_in_tx(&txn, project_id, item_id, FEEDBACK_REQUESTED_LABEL_KEY).await?;
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "item_finished", report)
        .await?;
    txn.commit().await.context("failed to commit item finish")?;
    events::publish_work_item_changed(project_name, item_id);

    model_to_view(store, updated).await
}

pub async fn delete_item(store: &Store, project_name: &str, item_id: i64) -> Result<()> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item delete")?;
    get_item_model_in_tx(&txn, project_id, item_id).await?;

    work_item_events::record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "item_deleted",
        "Deleted item",
    )
    .await?;
    WorkItem::delete_by_id(item_id)
        .exec(&txn)
        .await
        .context("failed to delete work item")?;
    txn.commit().await.context("failed to commit item delete")?;
    events::publish_work_item_changed(project_name, item_id);
    Ok(())
}

pub async fn list_item_labels(
    store: &Store,
    project_name: &str,
    item_id: i64,
) -> Result<Vec<WorkItemLabelView>> {
    let project_id = projects::project_id(store, project_name).await?;
    get_item_model(store, project_id, item_id).await?;
    labels_for_item(store.db().as_ref(), project_id, item_id).await
}

pub async fn list_project_labels(
    store: &Store,
    project_name: &str,
) -> Result<Vec<ProjectLabelView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let labels = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .order_by_asc(work_item_label::Column::Key)
        .order_by_asc(work_item_label::Column::Value)
        .all(store.db().as_ref())
        .await
        .context("failed to list project labels")?;

    let mut grouped = std::collections::BTreeMap::<(String, Option<String>), (i64, String)>::new();
    for label in labels {
        let key = (label.key, label.value);
        grouped
            .entry(key)
            .and_modify(|(count, last_used_at)| {
                *count += 1;
                if label.updated_at > *last_used_at {
                    *last_used_at = label.updated_at.clone();
                }
            })
            .or_insert((1, label.updated_at));
    }

    Ok(grouped
        .into_iter()
        .map(
            |((key, value), (usage_count, last_used_at))| ProjectLabelView {
                key,
                value,
                usage_count,
                last_used_at,
            },
        )
        .collect())
}

pub async fn add_label(
    store: &Store,
    project_name: &str,
    item_id: i64,
    key: String,
    value: Option<String>,
    expect_version: Option<i64>,
) -> Result<WorkItemView> {
    let key = item_labels::normalize_key(key)?;
    let value = item_labels::normalize_value(value);
    item_labels::validate_pair(&key, value.as_deref())?;
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start label add")?;
    let item = get_item_model_in_tx(&txn, project_id, item_id).await?;
    check_expected_version(expect_version, item.version)?;
    if WorkItemLabel::find()
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .filter(work_item_label::Column::Key.eq(&key))
        .one(&txn)
        .await
        .context("failed to check existing label")?
        .is_some()
    {
        bail!("item already has label '{key}'");
    }

    insert_label_in_tx(&txn, project_id, item_id, &key, value.as_deref()).await?;
    let updated = touch_item_in_tx(&txn, item).await?;
    let body = format!(
        "Added label {}",
        item_labels::format_label(&key, value.as_deref())
    );
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "label_added", &body)
        .await?;
    txn.commit().await.context("failed to commit label add")?;
    events::publish_work_item_changed(project_name, item_id);
    model_to_view(store, updated).await
}

pub async fn update_label(
    store: &Store,
    project_name: &str,
    item_id: i64,
    label_id: i64,
    key: Option<String>,
    value: Option<Option<String>>,
    expect_version: Option<i64>,
) -> Result<WorkItemView> {
    if key.is_none() && value.is_none() {
        bail!("label update requires at least one field");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start label update")?;
    let item = get_item_model_in_tx(&txn, project_id, item_id).await?;
    check_expected_version(expect_version, item.version)?;
    let existing = get_label_model_in_tx(&txn, project_id, item_id, label_id).await?;
    let key = match key {
        Some(key) => item_labels::normalize_key(key)?,
        None => existing.key.clone(),
    };
    let value = match value {
        Some(value) => item_labels::normalize_value(value),
        None => existing.value.clone(),
    };
    item_labels::validate_pair(&key, value.as_deref())?;
    if WorkItemLabel::find()
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .filter(work_item_label::Column::Key.eq(&key))
        .filter(work_item_label::Column::Id.ne(label_id))
        .one(&txn)
        .await
        .context("failed to check conflicting label")?
        .is_some()
    {
        bail!("item already has label '{key}'");
    }

    let mut active: WorkItemLabelActiveModel = existing.into();
    active.key = Set(key.clone());
    active.value = Set(value.clone());
    active.updated_at = Set(utc_now());
    active
        .update(&txn)
        .await
        .context("failed to update label")?;
    let updated = touch_item_in_tx(&txn, item).await?;
    let body = format!(
        "Updated label {}",
        item_labels::format_label(&key, value.as_deref())
    );
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "label_updated", &body)
        .await?;
    txn.commit()
        .await
        .context("failed to commit label update")?;
    events::publish_work_item_changed(project_name, item_id);
    model_to_view(store, updated).await
}

pub async fn delete_label(
    store: &Store,
    project_name: &str,
    item_id: i64,
    label_id: i64,
    expect_version: Option<i64>,
) -> Result<DeleteWorkItemLabelResponse> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start label delete")?;
    let item = get_item_model_in_tx(&txn, project_id, item_id).await?;
    check_expected_version(expect_version, item.version)?;
    let label = get_label_model_in_tx(&txn, project_id, item_id, label_id).await?;
    if label.key == STATE_LABEL_KEY {
        bail!("state label cannot be deleted; move the item to another state instead");
    }
    let body = format!(
        "Deleted label {}",
        item_labels::format_label(&label.key, label.value.as_deref())
    );
    WorkItemLabel::delete_by_id(label_id)
        .exec(&txn)
        .await
        .context("failed to delete label")?;
    let updated = touch_item_in_tx(&txn, item).await?;
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "label_deleted", &body)
        .await?;
    txn.commit()
        .await
        .context("failed to commit label delete")?;
    events::publish_work_item_changed(project_name, item_id);
    let work_item = model_to_view(store, updated).await?;
    Ok(DeleteWorkItemLabelResponse {
        deleted: true,
        label_id,
        work_item,
    })
}

pub async fn list_events(
    store: &Store,
    project_name: &str,
    item_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<WorkItemEventView>> {
    let project_id = projects::project_id(store, project_name).await?;
    if let Some(item_id) = item_id {
        get_item_model(store, project_id, item_id).await?;
    }

    let mut query = work_item_event::Entity::find()
        .filter(work_item_event::Column::ProjectId.eq(project_id))
        .order_by_asc(work_item_event::Column::Id);

    if let Some(item_id) = item_id {
        query = query.filter(work_item_event::Column::WorkItemId.eq(item_id));
    }

    if let Some(since_id) = since_id {
        query = query.filter(work_item_event::Column::Id.gt(since_id));
    }

    let events = query
        .all(store.db().as_ref())
        .await
        .context("failed to list work item events")?;

    Ok(events
        .into_iter()
        .map(|event| WorkItemEventView {
            id: event.id,
            project_id: event.project_id,
            work_item_id: event.work_item_id,
            event_type: event.event_type,
            body: event.body,
            actor_type: event.actor_type,
            actor_id: event.actor_id,
            agent_run_id: event.agent_run_id,
            created_at: event.created_at,
        })
        .collect())
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

pub(crate) async fn get_item_model(
    store: &Store,
    project_id: i64,
    item_id: i64,
) -> Result<WorkItemModel> {
    WorkItem::find_by_id(item_id)
        .filter(work_item::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context_with(|| format!("failed to load item {item_id}"))?
        .ok_or_else(|| report!("item {item_id} does not exist in this project"))
}

pub(crate) async fn get_item_model_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
) -> Result<WorkItemModel>
where
    C: sea_orm::ConnectionTrait,
{
    WorkItem::find_by_id(item_id)
        .filter(work_item::Column::ProjectId.eq(project_id))
        .one(conn)
        .await
        .context_with(|| format!("failed to load item {item_id}"))?
        .ok_or_else(|| report!("item {item_id} does not exist in this project"))
}

async fn get_label_model_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    label_id: i64,
) -> Result<WorkItemLabelModel>
where
    C: sea_orm::ConnectionTrait,
{
    WorkItemLabel::find_by_id(label_id)
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .one(conn)
        .await
        .context_with(|| format!("failed to load label {label_id}"))?
        .ok_or_else(|| report!("label {label_id} does not exist on item {item_id}"))
}

async fn claim_candidate_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    agent_id: &str,
    source_state: &str,
    finished_policy: FinishedClaimPolicy,
) -> Result<Option<WorkItemModel>>
where
    C: sea_orm::ConnectionTrait,
{
    let now = utc_now();
    let sql = match finished_policy {
        FinishedClaimPolicy::RejectFinished => {
            r#"
        UPDATE work_items
        SET claimed_by = ?3,
            claimed_at = ?4,
            claim_expires_at = NULL,
            finished_at = NULL,
            version = version + 1,
            updated_at = ?4
        WHERE id = ?2
          AND project_id = ?1
          AND claimed_by IS NULL
          AND finished_at IS NULL
        RETURNING id
        "#
        }
        FinishedClaimPolicy::AllowFinished => {
            r#"
        UPDATE work_items
        SET claimed_by = ?3,
            claimed_at = ?4,
            claim_expires_at = NULL,
            finished_at = NULL,
            version = version + 1,
            updated_at = ?4
        WHERE id = ?2
          AND project_id = ?1
          AND claimed_by IS NULL
        RETURNING id
        "#
        }
    };

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

    record_claimed_id_in_tx(conn, project_id, claimed_id, agent_id, source_state).await
}

async fn record_claimed_id_in_tx<C>(
    conn: &C,
    project_id: i64,
    claimed_id: Option<i64>,
    agent_id: &str,
    source_state: &str,
) -> Result<Option<WorkItemModel>>
where
    C: sea_orm::ConnectionTrait,
{
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
    Ok(Some(model_to_view(store, item).await?))
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
    upsert_label_in_tx(
        conn,
        project_id,
        item_id,
        CLAIMED_FROM_STATE_LABEL_KEY,
        Some(source_state),
    )
    .await?;
    upsert_label_in_tx(
        conn,
        project_id,
        item_id,
        STATE_LABEL_KEY,
        Some(CLAIMED_STATE_LABEL),
    )
    .await?;
    delete_label_by_key_in_tx(conn, project_id, item_id, FEEDBACK_REQUESTED_LABEL_KEY).await?;
    let comment_body = format!("Claimed by {agent_id}");
    insert_comment_in_tx(
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
    get_item_model_in_tx(conn, project_id, item_id).await
}

async fn item_ids_with_state<C>(conn: &C, project_id: i64, state: &str) -> Result<Vec<i64>>
where
    C: sea_orm::ConnectionTrait,
{
    let labels = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::Key.eq(STATE_LABEL_KEY))
        .filter(work_item_label::Column::Value.eq(state))
        .all(conn)
        .await
        .context_with(|| format!("failed to list items with state label '{state}'"))?;
    Ok(labels.into_iter().map(|label| label.work_item_id).collect())
}

async fn insert_label_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    key: &str,
    value: Option<&str>,
) -> Result<WorkItemLabelModel>
where
    C: sea_orm::ConnectionTrait,
{
    let now = utc_now();
    let active = WorkItemLabelActiveModel {
        project_id: Set(project_id),
        work_item_id: Set(item_id),
        key: Set(key.to_owned()),
        value: Set(value.map(ToOwned::to_owned)),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };
    Ok(active.insert(conn).await.context_with(|| {
        format!(
            "failed to add label '{}'",
            item_labels::format_label(key, value)
        )
    })?)
}

async fn upsert_label_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    key: &str,
    value: Option<&str>,
) -> Result<WorkItemLabelModel>
where
    C: sea_orm::ConnectionTrait,
{
    if let Some(existing) = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .filter(work_item_label::Column::Key.eq(key))
        .one(conn)
        .await
        .context_with(|| format!("failed to load label '{key}'"))?
    {
        let mut active: WorkItemLabelActiveModel = existing.into();
        active.value = Set(value.map(ToOwned::to_owned));
        active.updated_at = Set(utc_now());
        Ok(active
            .update(conn)
            .await
            .context_with(|| format!("failed to update label '{key}'"))?)
    } else {
        insert_label_in_tx(conn, project_id, item_id, key, value).await
    }
}

async fn delete_label_by_key_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    key: &str,
) -> Result<()>
where
    C: sea_orm::ConnectionTrait,
{
    WorkItemLabel::delete_many()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .filter(work_item_label::Column::Key.eq(key))
        .exec(conn)
        .await
        .context_with(|| format!("failed to delete label '{key}'"))?;
    Ok(())
}

async fn labels_for_item<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
) -> Result<Vec<WorkItemLabelView>>
where
    C: sea_orm::ConnectionTrait,
{
    let labels = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .order_by_asc(work_item_label::Column::Key)
        .order_by_asc(work_item_label::Column::Value)
        .all(conn)
        .await
        .context("failed to list item labels")?;
    Ok(labels.into_iter().map(label_to_view).collect())
}

async fn touch_item_in_tx<C>(conn: &C, item: WorkItemModel) -> Result<WorkItemModel>
where
    C: sea_orm::ConnectionTrait,
{
    let version = item.version;
    let mut active: WorkItemActiveModel = item.into();
    active.version = Set(version + 1);
    active.updated_at = Set(utc_now());
    Ok(active
        .update(conn)
        .await
        .context("failed to update item version")?)
}

async fn insert_comment_in_tx<C>(
    conn: &C,
    item_id: i64,
    author_type: AuthorType,
    author_name: Option<String>,
    body: &str,
) -> Result<CommentModel>
where
    C: sea_orm::ConnectionTrait,
{
    let active = CommentActiveModel {
        work_item_id: Set(item_id),
        author_type: Set(author_type.as_storage().to_owned()),
        author_name: Set(author_name),
        body: Set(body.to_owned()),
        created_at: Set(utc_now()),
        ..Default::default()
    };
    Ok(active
        .insert(conn)
        .await
        .context("failed to add item comment")?)
}

async fn models_to_views(
    store: &Store,
    project_id: i64,
    items: Vec<WorkItemModel>,
) -> Result<Vec<WorkItemView>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let item_ids = items.iter().map(|item| item.id).collect::<Vec<_>>();
    let mut labels = labels_for_items(store.db().as_ref(), project_id, &item_ids).await?;
    let mut comment_counts = comment_counts_for_items(store.db().as_ref(), &item_ids).await?;
    let mut claim_sources =
        claim_sources_for_items(store.db().as_ref(), project_id, &items).await?;

    let mut views = Vec::with_capacity(items.len());
    for item in items {
        let item_id = item.id;
        views.push(work_item_view(
            item,
            labels.remove(&item_id).unwrap_or_default(),
            comment_counts.remove(&item_id).unwrap_or(0),
            claim_sources.remove(&item_id),
        )?);
    }
    Ok(views)
}

async fn model_to_view(store: &Store, item: WorkItemModel) -> Result<WorkItemView> {
    let labels = labels_for_item(store.db().as_ref(), item.project_id, item.id).await?;
    let comment_count = comment_counts_for_items(store.db().as_ref(), &[item.id])
        .await?
        .remove(&item.id)
        .unwrap_or(0);
    let mut claim_sources = claim_sources_for_items(
        store.db().as_ref(),
        item.project_id,
        std::slice::from_ref(&item),
    )
    .await?;
    let claim_source = claim_sources.remove(&item.id);
    work_item_view(item, labels, comment_count, claim_source)
}

async fn labels_for_items<C>(
    conn: &C,
    project_id: i64,
    item_ids: &[i64],
) -> Result<BTreeMap<i64, Vec<WorkItemLabelView>>>
where
    C: sea_orm::ConnectionTrait,
{
    let labels = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.is_in(item_ids.iter().copied()))
        .order_by_asc(work_item_label::Column::WorkItemId)
        .order_by_asc(work_item_label::Column::Key)
        .order_by_asc(work_item_label::Column::Value)
        .all(conn)
        .await
        .context("failed to list item labels")?;

    let mut labels_by_item = BTreeMap::<i64, Vec<WorkItemLabelView>>::new();
    for label in labels {
        labels_by_item
            .entry(label.work_item_id)
            .or_default()
            .push(label_to_view(label));
    }
    Ok(labels_by_item)
}

async fn comment_counts_for_items<C>(conn: &C, item_ids: &[i64]) -> Result<BTreeMap<i64, i64>>
where
    C: sea_orm::ConnectionTrait,
{
    if item_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let placeholders = (1..=item_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let values = item_ids.iter().copied().map(Into::into).collect::<Vec<_>>();
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            format!(
                r#"
                SELECT work_item_id, COUNT(*) AS count
                FROM comments
                WHERE work_item_id IN ({placeholders})
                GROUP BY work_item_id
                "#
            ),
            values,
        ))
        .await
        .context("failed to count item comments")?;

    let mut counts = BTreeMap::new();
    for row in rows {
        counts.insert(
            row.try_get::<i64>("", "work_item_id")
                .context("failed to read comment count item id")?,
            row.try_get::<i64>("", "count")
                .context("failed to read item comment count")?,
        );
    }
    Ok(counts)
}

async fn claim_sources_for_items<C>(
    conn: &C,
    project_id: i64,
    items: &[WorkItemModel],
) -> Result<BTreeMap<i64, WorkItemClaimSourceView>>
where
    C: sea_orm::ConnectionTrait,
{
    let run_to_item = items
        .iter()
        .filter_map(|item| {
            let run_id = patchbay_run_id_from_agent(item.claimed_by.as_deref()?)?;
            Some((run_id, item.id))
        })
        .collect::<BTreeMap<_, _>>();
    if run_to_item.is_empty() {
        return Ok(BTreeMap::new());
    }

    let run_ids = run_to_item.keys().copied().collect::<Vec<_>>();
    let runs = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .filter(agent_run::Column::Id.is_in(run_ids))
        .all(conn)
        .await
        .context("failed to list claimed item agent runs")?;

    let mut claim_sources = BTreeMap::new();
    for run in runs {
        let Some(item_id) = run_to_item.get(&run.id).copied() else {
            continue;
        };
        if run.work_item_id != Some(item_id) {
            continue;
        }
        claim_sources.insert(item_id, claim_source_from_run(run));
    }

    Ok(claim_sources)
}

fn claim_source_from_run(run: AgentRunModel) -> WorkItemClaimSourceView {
    WorkItemClaimSourceView {
        run_id: run.id,
        trigger_id: run.trigger_id,
        trigger_name: projects::normalize_optional(run.trigger_name),
    }
}

fn patchbay_run_id_from_agent(agent_id: &str) -> Option<i64> {
    agent_id.strip_prefix("patchbay-run-")?.parse().ok()
}

fn work_item_view(
    item: WorkItemModel,
    labels: Vec<WorkItemLabelView>,
    comment_count: i64,
    claim_source: Option<WorkItemClaimSourceView>,
) -> Result<WorkItemView> {
    let state = item_labels::current_state(&labels);

    Ok(WorkItemView {
        id: item.id,
        project_id: item.project_id,
        title: item.title,
        description: item.description,
        state,
        labels,
        version: item.version,
        claimed_by: item.claimed_by,
        claimed_at: item.claimed_at,
        claim_expires_at: item.claim_expires_at,
        claim_source,
        finished_at: item.finished_at,
        agent_model_override: projects::normalize_optional(item.agent_model_override),
        agent_reasoning_effort_override: item
            .agent_reasoning_effort_override
            .as_deref()
            .map(str::parse::<AgentReasoningEffort>)
            .transpose()?,
        created_at: item.created_at,
        updated_at: item.updated_at,
        comment_count,
    })
}

fn label_to_view(label: WorkItemLabelModel) -> WorkItemLabelView {
    WorkItemLabelView {
        id: label.id,
        project_id: label.project_id,
        work_item_id: label.work_item_id,
        key: label.key,
        value: label.value,
        created_at: label.created_at,
        updated_at: label.updated_at,
    }
}

fn validate_item_text(title: &str, description: &str) -> Result<()> {
    if title.trim().is_empty() {
        bail!("item title cannot be empty");
    }
    if description.trim().is_empty() {
        bail!("item description cannot be empty");
    }
    Ok(())
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

fn check_expected_version(expected: Option<i64>, actual: i64) -> Result<()> {
    if let Some(expected) = expected
        && expected != actual
    {
        bail!("version conflict: expected {expected}, found {actual}");
    }
    Ok(())
}

fn timestamp_is_before_or_equal(value: &str, cutoff: OffsetDateTime) -> bool {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(|timestamp| timestamp <= cutoff)
        .unwrap_or(false)
}

fn comment_to_view(comment: CommentModel) -> Result<CommentView> {
    Ok(CommentView {
        id: comment.id,
        work_item_id: comment.work_item_id,
        author_type: AuthorType::from_str(&comment.author_type)?,
        author_name: comment.author_name,
        body: comment.body,
        created_at: comment.created_at,
    })
}

#[cfg(test)]
mod tests {
    use crudkit_core::condition::{
        Condition, ConditionClause, ConditionClauseValue, ConditionElement, Operator,
    };
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        comments::{AddComment, add_comment, list_comments},
        projects::{CreateProject, create_project},
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
    async fn work_items_are_scoped_to_project() {
        let (_temp, store) = test_store().await;
        let demo_item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Demo item".to_owned(),
                description: "Build the demo item".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        create_item(
            &store,
            "other",
            CreateWorkItem {
                title: "Other item".to_owned(),
                description: "Build the other item".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let demo_items = list_items(&store, "demo", None).await.unwrap();
        let other_lookup = get_item(&store, "other", demo_item.id).await.unwrap_err();

        assert_eq!(demo_items.len(), 1);
        assert_eq!(demo_items[0].title, "Demo item");
        assert!(
            other_lookup
                .to_string()
                .contains("does not exist in this project")
        );
    }

    #[tokio::test]
    async fn creating_item_records_item_created_event_row() {
        let (_temp, store) = test_store().await;
        let project_id = projects::project_id(&store, "demo").await.unwrap();

        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Record event".to_owned(),
                description: "Persist the creation event".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].project_id, project_id);
        assert_eq!(events[0].work_item_id, Some(item.id));
        assert_eq!(events[0].event_type, "item_created");
        assert_eq!(events[0].body, "Created item");
        assert!(events[0].actor_type.is_none());
        assert!(events[0].actor_id.is_none());
        assert!(events[0].agent_run_id.is_none());
    }

    #[tokio::test]
    async fn list_items_hydrates_labels_state_and_comment_counts() {
        let (_temp, store) = test_store().await;
        let first = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "First item".to_owned(),
                description: "Has several comments and labels".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        let second = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Second item".to_owned(),
                description: "Has an independent label and comment count".to_owned(),
                state: "ready".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            first.id,
            "severity".to_owned(),
            Some("high".to_owned()),
            None,
        )
        .await
        .unwrap();
        add_label(&store, "demo", second.id, "bug".to_owned(), None, None)
            .await
            .unwrap();

        for body in ["First comment", "Second comment"] {
            add_comment(
                &store,
                "demo",
                first.id,
                AddComment {
                    author_type: AuthorType::User,
                    author_name: Some("operator".to_owned()),
                    body: body.to_owned(),
                },
            )
            .await
            .unwrap();
        }
        add_comment(
            &store,
            "demo",
            second.id,
            AddComment {
                author_type: AuthorType::Agent,
                author_name: Some("agent-a".to_owned()),
                body: "Only comment".to_owned(),
            },
        )
        .await
        .unwrap();

        let items = list_items(&store, "demo", None).await.unwrap();
        let first = items.iter().find(|item| item.id == first.id).unwrap();
        let second = items.iter().find(|item| item.id == second.id).unwrap();

        assert_eq!(first.state.as_deref(), Some("open"));
        assert_eq!(first.comment_count, 2);
        assert!(
            first
                .labels
                .iter()
                .any(|label| { label.key == "severity" && label.value.as_deref() == Some("high") })
        );
        assert_eq!(second.state.as_deref(), Some("ready"));
        assert_eq!(second.comment_count, 1);
        assert!(
            second
                .labels
                .iter()
                .any(|label| { label.key == "bug" && label.value.is_none() })
        );
    }

    #[tokio::test]
    async fn counts_items_outside_authored_work_item_states() {
        let (_temp, store) = test_store().await;
        create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Valid item".to_owned(),
                description: "Uses an authored state".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Invalid item".to_owned(),
                description: "Uses an unconfigured state".to_owned(),
                state: "needs_triage".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        let unlabeled = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Unlabeled item".to_owned(),
                description: "Has no state label".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        create_item(
            &store,
            "other",
            CreateWorkItem {
                title: "Other project invalid item".to_owned(),
                description: "Should not affect the demo count".to_owned(),
                state: "needs_triage".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        let project_id = projects::project_id(&store, "demo").await.unwrap();
        delete_label_by_key_in_tx(
            store.db().as_ref(),
            project_id,
            unlabeled.id,
            STATE_LABEL_KEY,
        )
        .await
        .unwrap();

        let demo_count = count_items_outside_work_item_states(&store, "demo")
            .await
            .unwrap();
        let other_count = count_items_outside_work_item_states(&store, "other")
            .await
            .unwrap();

        assert_eq!(demo_count, 2);
        assert_eq!(other_count, 1);
    }

    #[tokio::test]
    async fn work_items_read_view_exposes_state_label() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Visible state".to_owned(),
                description: "Shows state in the CrudKit read view".to_owned(),
                state: "needs_triage".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        let project_id = projects::project_id(&store, "demo").await.unwrap();

        let row = store
            .db()
            .query_one(Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                r#"
                SELECT "state_label"
                FROM "work_items_read_view"
                WHERE "project_id" = ?1
                  AND "id" = ?2
                "#,
                vec![project_id.into(), item.id.into()],
            ))
            .await
            .unwrap()
            .unwrap();
        let state_label = row.try_get::<Option<String>>("", "state_label").unwrap();

        assert_eq!(state_label.as_deref(), Some("needs_triage"));
    }

    #[tokio::test]
    async fn moving_item_updates_state_and_version() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Move me".to_owned(),
                description: "Move through states".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let moved = move_item(
            &store,
            "demo",
            item.id,
            "in_progress".to_owned(),
            Some(item.version),
        )
        .await
        .unwrap();

        assert_eq!(moved.state.as_deref(), Some("in_progress"));
        assert_eq!(moved.version, item.version + 1);
    }

    #[tokio::test]
    async fn updating_item_fields_and_state_is_one_versioned_change() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Update me".to_owned(),
                description: "Move and edit together".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let updated = update_item(
            &store,
            "demo",
            item.id,
            UpdateWorkItem {
                title: Some("Updated title".to_owned()),
                description: None,
                state: Some("review".to_owned()),
                agent_model_override: Some(Some("gpt-5.4".to_owned())),
                agent_reasoning_effort_override: None,
                expect_version: Some(item.version),
            },
        )
        .await
        .unwrap();
        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(updated.title, "Updated title");
        assert_eq!(updated.state.as_deref(), Some("review"));
        assert_eq!(updated.agent_model_override.as_deref(), Some("gpt-5.4"));
        assert_eq!(updated.version, item.version + 1);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "item_updated")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "item_moved")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn stale_expected_version_is_rejected() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Update me".to_owned(),
                description: "Expect conflict".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let err = update_item(
            &store,
            "demo",
            item.id,
            UpdateWorkItem {
                title: Some("Changed".to_owned()),
                description: None,
                state: None,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                expect_version: Some(item.version + 1),
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("version conflict"));
    }

    #[tokio::test]
    async fn delete_removes_item_from_lists() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Delete me".to_owned(),
                description: "Hide after deletion".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        delete_item(&store, "demo", item.id).await.unwrap();

        assert!(list_items(&store, "demo", None).await.unwrap().is_empty());
        assert!(get_item(&store, "demo", item.id).await.is_err());
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
            has_unclaimed_item_matching_condition(&store, "demo", &selector)
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
            !has_unclaimed_item_matching_condition(&store, "demo", &selector)
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
        let agent_id = format!("patchbay-run-{}", run.id);

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
        let agent_id = format!("patchbay-run-{}", run.id);

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
    }

    #[tokio::test]
    async fn new_claims_overwrite_stale_claim_source_with_current_state() {
        let (_temp, store) = test_store().await;
        let selector_item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Selector retry".to_owned(),
                description: "Claim source should come from the current state label".to_owned(),
                state: "ready".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
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
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", "open")
            .await
            .unwrap()
            .unwrap();
        let project_id = projects::project_id(&store, "demo").await.unwrap();
        let mut model: WorkItemActiveModel = get_item_model(&store, project_id, item.id)
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
