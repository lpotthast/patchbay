use std::collections::BTreeMap;

use crudkit_core::condition::Condition;
use rootcause::{Result, prelude::*};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder,
    Statement, TransactionTrait,
};

use crate::{
    backend::{
        agent_ids,
        entities::work_item::{self, WorkItem, WorkItemModel},
        events, label_conditions, projects,
        storage::{Store, utc_now},
        work_item_comments, work_item_events, work_item_labels, work_items, workflow_labels,
    },
    shared::view_models::{WorkItemLabelView, WorkItemView},
};

pub(crate) async fn has_claimable_item_matching_condition(
    store: &Store,
    project_name: &str,
    condition: &Condition,
) -> Result<bool> {
    let selector = ClaimSelector::automation_condition(condition)?;
    let project_id = projects::project_id(store, project_name).await?;
    has_matching_candidate(store.db().as_ref(), project_id, &selector).await
}

pub(crate) async fn claim_item(
    store: &Store,
    project_name: &str,
    agent_id: &str,
    state_filter: &str,
) -> Result<Option<WorkItemView>> {
    agent_ids::validate_agent_id(agent_id)?;
    let selector = ClaimSelector::state(state_filter)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item claim")?;
    let item = claim_first_matching_candidate_in_tx(&txn, project_id, agent_id, &selector).await?;

    commit_claim_transaction(
        store,
        project_name,
        txn,
        item,
        "failed to commit item claim",
    )
    .await
}

pub(crate) async fn claim_item_matching_condition(
    store: &Store,
    project_name: &str,
    agent_id: &str,
    condition: &Condition,
) -> Result<Option<WorkItemView>> {
    agent_ids::validate_agent_id(agent_id)?;
    let selector = ClaimSelector::automation_condition(condition)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item claim")?;
    let item = claim_first_matching_candidate_in_tx(&txn, project_id, agent_id, &selector).await?;

    commit_claim_transaction(
        store,
        project_name,
        txn,
        item,
        "failed to commit item claim",
    )
    .await
}

pub(crate) async fn claim_specific_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
) -> Result<Option<WorkItemView>> {
    agent_ids::validate_agent_id(agent_id)?;

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
    let source_state = workflow_labels::source_state_for_new_claim(&labels);
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

async fn claim_first_matching_candidate_in_tx<C>(
    conn: &C,
    project_id: i64,
    agent_id: &str,
    selector: &ClaimSelector,
) -> Result<Option<WorkItemModel>>
where
    C: ConnectionTrait,
{
    let candidates = matching_candidates_in_claim_order(conn, project_id, selector).await?;
    for candidate in candidates {
        let claimed = claim_candidate_in_tx(
            conn,
            project_id,
            candidate.item_id,
            agent_id,
            &candidate.source_state,
        )
        .await?;

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
    C: ConnectionTrait,
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
    C: ConnectionTrait,
{
    workflow_labels::apply_plan_in_tx(
        conn,
        project_id,
        item_id,
        workflow_labels::new_claim_workflow_label_plan(source_state),
    )
    .await?;
    let comment_body = format!("Claimed by {agent_id}");
    work_item_comments::insert_system_in_tx(conn, item_id, comment_body.as_str()).await?;
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

struct ClaimCandidate {
    item_id: i64,
    source_state: String,
}

enum ClaimSelector {
    State(String),
    AutomationCondition(label_conditions::ValidatedLabelCondition),
}

impl ClaimSelector {
    fn state(state: impl Into<String>) -> Result<Self> {
        Ok(Self::State(workflow_labels::normalize_state_value(state)?))
    }

    fn automation_condition(condition: &Condition) -> Result<Self> {
        Ok(Self::AutomationCondition(
            label_conditions::ValidatedLabelCondition::new(condition)?,
        ))
    }

    fn matches(&self, labels: &[WorkItemLabelView]) -> bool {
        match self {
            Self::State(state) => {
                !workflow_labels::is_automation_blocked(labels)
                    && workflow_labels::current_state(labels).as_deref() == Some(state.as_str())
            }
            Self::AutomationCondition(selector) => selector.matches_automation_selector(labels),
        }
    }
}

async fn has_matching_candidate<C>(
    conn: &C,
    project_id: i64,
    selector: &ClaimSelector,
) -> Result<bool>
where
    C: ConnectionTrait,
{
    Ok(
        matching_candidates_in_claim_order(conn, project_id, selector)
            .await?
            .into_iter()
            .next()
            .is_some(),
    )
}

async fn matching_candidates_in_claim_order<C>(
    conn: &C,
    project_id: i64,
    selector: &ClaimSelector,
) -> Result<Vec<ClaimCandidate>>
where
    C: ConnectionTrait,
{
    let candidates = claimable_items_in_claim_order(conn, project_id).await?;
    let labels_by_item = labels_for_candidate_items(conn, project_id, &candidates).await?;
    let mut matching = Vec::new();

    for candidate in candidates {
        let labels = labels_for_item(&labels_by_item, candidate.id);
        if !selector.matches(labels) {
            continue;
        }

        matching.push(ClaimCandidate {
            item_id: candidate.id,
            source_state: workflow_labels::source_state_for_new_claim(labels),
        });
    }

    Ok(matching)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::view_models::{
        AUTOMATION_BLOCKED_LABEL_KEY, FEEDBACK_REQUESTED_LABEL_KEY, STATE_LABEL_KEY,
    };

    fn label(key: &str, value: Option<&str>) -> WorkItemLabelView {
        WorkItemLabelView {
            id: 1,
            project_id: 1,
            work_item_id: 1,
            key: key.to_owned(),
            value: value.map(ToOwned::to_owned),
            created_at: "2026-06-18T00:00:00Z".to_owned(),
            updated_at: "2026-06-18T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn state_selector_matches_current_state_and_skips_workflow_blockers() {
        let selector = ClaimSelector::state(" open ").unwrap();

        assert!(selector.matches(&[label(STATE_LABEL_KEY, Some("open"))]));
        assert!(!selector.matches(&[label(STATE_LABEL_KEY, Some("idea"))]));
        assert!(!selector.matches(&[
            label(STATE_LABEL_KEY, Some("open")),
            label(AUTOMATION_BLOCKED_LABEL_KEY, None),
        ]));
        assert!(!selector.matches(&[
            label(STATE_LABEL_KEY, Some("open")),
            label(FEEDBACK_REQUESTED_LABEL_KEY, None),
        ]));
    }
}
