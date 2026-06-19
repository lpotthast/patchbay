use std::collections::BTreeMap;

use crudkit_core::condition::Condition;
use rootcause::{Result, prelude::*};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::{
    backend::{
        entities::work_item::{self, WorkItem, WorkItemModel},
        item_labels, label_conditions, work_item_labels, workflow_labels,
    },
    shared::view_models::WorkItemLabelView,
};

pub(crate) struct ClaimCandidate {
    pub(crate) item_id: i64,
    pub(crate) source_state: String,
}

pub(crate) enum ClaimSelector {
    State(String),
    AutomationCondition(label_conditions::ValidatedLabelCondition),
}

impl ClaimSelector {
    pub(crate) fn state(state: impl Into<String>) -> Result<Self> {
        Ok(Self::State(item_labels::normalize_state_value(state)?))
    }

    pub(crate) fn automation_condition(condition: &Condition) -> Result<Self> {
        Ok(Self::AutomationCondition(
            label_conditions::ValidatedLabelCondition::new(condition)?,
        ))
    }

    fn matches(&self, labels: &[WorkItemLabelView]) -> bool {
        match self {
            Self::State(state) => {
                !workflow_labels::is_automation_blocked(labels)
                    && item_labels::current_state(labels).as_deref() == Some(state.as_str())
            }
            Self::AutomationCondition(selector) => selector.matches_automation_selector(labels),
        }
    }
}

pub(crate) async fn has_matching_candidate<C>(
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

pub(crate) async fn matching_candidates_in_claim_order<C>(
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
