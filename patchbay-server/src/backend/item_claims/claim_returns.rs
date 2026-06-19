use rootcause::{Result, prelude::*};
use sea_orm::{ActiveModelTrait, TransactionTrait};

use crate::{
    backend::{
        agent_ids, events, projects,
        storage::{Store, utc_now},
        work_item_comments, work_item_events, work_item_labels, work_items, workflow_labels,
    },
    shared::view_models::WorkItemView,
};

use super::active_claims;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReleaseAutomationDisposition {
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

    fn label_disposition(&self) -> workflow_labels::ClaimReturnLabelDisposition {
        match self {
            Self::Release {
                automation_disposition,
                ..
            } => match automation_disposition {
                ReleaseAutomationDisposition::Claimable => {
                    workflow_labels::ClaimReturnLabelDisposition::ClaimableRelease
                }
                ReleaseAutomationDisposition::Blocked => {
                    workflow_labels::ClaimReturnLabelDisposition::BlockedRelease
                }
            },
            Self::FeedbackRequest { .. } => {
                workflow_labels::ClaimReturnLabelDisposition::FeedbackRequest
            }
        }
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

pub(crate) async fn release_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    comment: Option<String>,
    automation_disposition: ReleaseAutomationDisposition,
) -> Result<WorkItemView> {
    agent_ids::validate_agent_id(agent_id)?;
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

pub(crate) async fn request_feedback(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    body: &str,
) -> Result<WorkItemView> {
    agent_ids::validate_agent_id(agent_id)?;
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

async fn return_claim_to_source_state(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    mode: ClaimReturnMode<'_>,
) -> Result<WorkItemView> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store.db().begin().await.context(mode.start_context())?;
    let claim = active_claims::load_in_tx(&txn, project_id, item_id, agent_id).await?;
    let labels = work_item_labels::for_item(&txn, project_id, item_id).await?;
    let release_state = workflow_labels::release_state_from_claim_labels(&labels);

    let active = claim.clear_active_model(utc_now());
    let updated = active.update(&txn).await.context(mode.update_context())?;

    workflow_labels::apply_plan_in_tx(
        &txn,
        project_id,
        item_id,
        workflow_labels::claim_return_workflow_label_plan(&release_state, mode.label_disposition()),
    )
    .await?;

    if let Some(comment) = mode
        .agent_comment_body()
        .filter(|body| !body.trim().is_empty())
    {
        work_item_comments::insert_agent_in_tx(&txn, item_id, agent_id, comment).await?;
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
