use crate::{
    backend::{item_labels, work_item_labels},
    shared::view_models::{
        AUTOMATION_BLOCKED_LABEL_KEY, CLAIMED_FROM_STATE_LABEL_KEY, CLAIMED_STATE_LABEL,
        DEFAULT_STATE_LABEL, FEEDBACK_REQUESTED_LABEL_KEY, FINISHED_STATE_LABEL, STATE_LABEL_KEY,
        WorkItemLabelView,
    },
};
use rootcause::Result;
use sea_orm::ConnectionTrait;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClaimReturnLabelDisposition {
    ClaimableRelease,
    BlockedRelease,
    FeedbackRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkflowLabelPlan<'a> {
    pub(crate) upserts: Vec<WorkflowLabelUpsert<'a>>,
    pub(crate) delete_keys: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkflowLabelUpsert<'a> {
    pub(crate) key: &'static str,
    pub(crate) value: Option<&'a str>,
}

const NEW_CLAIM_DELETES_WORKFLOW_LABELS: &[&str] = &[FEEDBACK_REQUESTED_LABEL_KEY];
const FINISH_DELETES_WORKFLOW_LABELS: &[&str] = &[
    CLAIMED_FROM_STATE_LABEL_KEY,
    AUTOMATION_BLOCKED_LABEL_KEY,
    FEEDBACK_REQUESTED_LABEL_KEY,
];
const CLAIMABLE_RELEASE_DELETES_WORKFLOW_LABELS: &[&str] = FINISH_DELETES_WORKFLOW_LABELS;
const BLOCKED_RELEASE_DELETES_WORKFLOW_LABELS: &[&str] =
    &[CLAIMED_FROM_STATE_LABEL_KEY, FEEDBACK_REQUESTED_LABEL_KEY];
const FEEDBACK_REQUEST_DELETES_WORKFLOW_LABELS: &[&str] = &[CLAIMED_FROM_STATE_LABEL_KEY];

pub(crate) fn is_automation_blocked(labels: &[WorkItemLabelView]) -> bool {
    labels.iter().any(|label| {
        label.key == AUTOMATION_BLOCKED_LABEL_KEY || label.key == FEEDBACK_REQUESTED_LABEL_KEY
    })
}

pub(crate) fn source_state_for_new_claim(labels: &[WorkItemLabelView]) -> String {
    item_labels::current_state(labels).unwrap_or_else(|| DEFAULT_STATE_LABEL.to_owned())
}

pub(crate) fn release_state_from_claim_labels(labels: &[WorkItemLabelView]) -> String {
    labels
        .iter()
        .find(|label| label.key == CLAIMED_FROM_STATE_LABEL_KEY)
        .and_then(|label| label.value.clone())
        .or_else(|| item_labels::current_state(labels))
        .unwrap_or_else(|| DEFAULT_STATE_LABEL.to_owned())
}

pub(crate) fn new_claim_workflow_label_plan(source_state: &str) -> WorkflowLabelPlan<'_> {
    WorkflowLabelPlan {
        upserts: vec![
            WorkflowLabelUpsert {
                key: CLAIMED_FROM_STATE_LABEL_KEY,
                value: Some(source_state),
            },
            WorkflowLabelUpsert {
                key: STATE_LABEL_KEY,
                value: Some(CLAIMED_STATE_LABEL),
            },
        ],
        delete_keys: NEW_CLAIM_DELETES_WORKFLOW_LABELS,
    }
}

pub(crate) fn finish_workflow_label_plan() -> WorkflowLabelPlan<'static> {
    WorkflowLabelPlan {
        upserts: vec![WorkflowLabelUpsert {
            key: STATE_LABEL_KEY,
            value: Some(FINISHED_STATE_LABEL),
        }],
        delete_keys: FINISH_DELETES_WORKFLOW_LABELS,
    }
}

pub(crate) fn claim_return_workflow_label_plan(
    release_state: &str,
    disposition: ClaimReturnLabelDisposition,
) -> WorkflowLabelPlan<'_> {
    let delete_keys = match disposition {
        ClaimReturnLabelDisposition::ClaimableRelease => CLAIMABLE_RELEASE_DELETES_WORKFLOW_LABELS,
        ClaimReturnLabelDisposition::BlockedRelease => BLOCKED_RELEASE_DELETES_WORKFLOW_LABELS,
        ClaimReturnLabelDisposition::FeedbackRequest => FEEDBACK_REQUEST_DELETES_WORKFLOW_LABELS,
    };
    let mut upserts = vec![WorkflowLabelUpsert {
        key: STATE_LABEL_KEY,
        value: Some(release_state),
    }];
    match disposition {
        ClaimReturnLabelDisposition::ClaimableRelease => {}
        ClaimReturnLabelDisposition::BlockedRelease => {
            upserts.push(WorkflowLabelUpsert {
                key: AUTOMATION_BLOCKED_LABEL_KEY,
                value: None,
            });
        }
        ClaimReturnLabelDisposition::FeedbackRequest => {
            upserts.push(WorkflowLabelUpsert {
                key: AUTOMATION_BLOCKED_LABEL_KEY,
                value: None,
            });
            upserts.push(WorkflowLabelUpsert {
                key: FEEDBACK_REQUESTED_LABEL_KEY,
                value: None,
            });
        }
    }

    WorkflowLabelPlan {
        upserts,
        delete_keys,
    }
}

pub(crate) async fn apply_plan_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    plan: WorkflowLabelPlan<'_>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    for label_key in plan.delete_keys {
        work_item_labels::delete_by_key_in_tx(conn, project_id, item_id, label_key).await?;
    }
    for upsert in plan.upserts {
        work_item_labels::upsert_in_tx(conn, project_id, item_id, upsert.key, upsert.value).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn automation_blocking_recognizes_release_and_feedback_labels() {
        assert!(is_automation_blocked(&[label(
            AUTOMATION_BLOCKED_LABEL_KEY,
            None
        )]));
        assert!(is_automation_blocked(&[label(
            FEEDBACK_REQUESTED_LABEL_KEY,
            None
        )]));
        assert!(!is_automation_blocked(&[label(
            STATE_LABEL_KEY,
            Some("open")
        )]));
    }

    #[test]
    fn release_state_prefers_claim_source_then_current_state_then_default() {
        let labels = vec![
            label(STATE_LABEL_KEY, Some("in_progress")),
            label(CLAIMED_FROM_STATE_LABEL_KEY, Some("review")),
        ];
        assert_eq!(release_state_from_claim_labels(&labels), "review");

        let labels = vec![label(STATE_LABEL_KEY, Some("triage"))];
        assert_eq!(release_state_from_claim_labels(&labels), "triage");

        assert_eq!(release_state_from_claim_labels(&[]), DEFAULT_STATE_LABEL);
    }

    #[test]
    fn new_claim_label_plan_records_source_state_and_clears_feedback_wait() {
        let plan = new_claim_workflow_label_plan("review");

        assert_eq!(
            plan.upserts,
            vec![
                WorkflowLabelUpsert {
                    key: CLAIMED_FROM_STATE_LABEL_KEY,
                    value: Some("review"),
                },
                WorkflowLabelUpsert {
                    key: STATE_LABEL_KEY,
                    value: Some(CLAIMED_STATE_LABEL),
                },
            ]
        );
        assert_eq!(plan.delete_keys, [FEEDBACK_REQUESTED_LABEL_KEY]);
    }

    #[test]
    fn finish_label_plan_closes_item_and_clears_workflow_bookkeeping() {
        let plan = finish_workflow_label_plan();

        assert_eq!(
            plan.upserts,
            vec![WorkflowLabelUpsert {
                key: STATE_LABEL_KEY,
                value: Some(FINISHED_STATE_LABEL),
            }]
        );
        assert_eq!(
            plan.delete_keys,
            [
                CLAIMED_FROM_STATE_LABEL_KEY,
                AUTOMATION_BLOCKED_LABEL_KEY,
                FEEDBACK_REQUESTED_LABEL_KEY,
            ]
        );
    }

    #[test]
    fn claim_return_label_plans_capture_release_feedback_and_retry_policy() {
        let claimable =
            claim_return_workflow_label_plan("open", ClaimReturnLabelDisposition::ClaimableRelease);
        assert_eq!(
            claimable.upserts,
            vec![WorkflowLabelUpsert {
                key: STATE_LABEL_KEY,
                value: Some("open"),
            }]
        );
        assert_eq!(
            claimable.delete_keys,
            [
                CLAIMED_FROM_STATE_LABEL_KEY,
                AUTOMATION_BLOCKED_LABEL_KEY,
                FEEDBACK_REQUESTED_LABEL_KEY,
            ]
        );

        let blocked =
            claim_return_workflow_label_plan("ready", ClaimReturnLabelDisposition::BlockedRelease);
        assert_eq!(
            blocked.upserts,
            vec![
                WorkflowLabelUpsert {
                    key: STATE_LABEL_KEY,
                    value: Some("ready"),
                },
                WorkflowLabelUpsert {
                    key: AUTOMATION_BLOCKED_LABEL_KEY,
                    value: None,
                },
            ]
        );
        assert_eq!(
            blocked.delete_keys,
            [CLAIMED_FROM_STATE_LABEL_KEY, FEEDBACK_REQUESTED_LABEL_KEY]
        );

        let feedback = claim_return_workflow_label_plan(
            "triage",
            ClaimReturnLabelDisposition::FeedbackRequest,
        );
        assert_eq!(
            feedback.upserts,
            vec![
                WorkflowLabelUpsert {
                    key: STATE_LABEL_KEY,
                    value: Some("triage"),
                },
                WorkflowLabelUpsert {
                    key: AUTOMATION_BLOCKED_LABEL_KEY,
                    value: None,
                },
                WorkflowLabelUpsert {
                    key: FEEDBACK_REQUESTED_LABEL_KEY,
                    value: None,
                },
            ]
        );
        assert_eq!(feedback.delete_keys, [CLAIMED_FROM_STATE_LABEL_KEY]);
    }
}
