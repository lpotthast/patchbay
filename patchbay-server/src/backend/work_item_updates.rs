use rootcause::{Result, prelude::*};
use sea_orm::ActiveValue::Set;

use crate::{
    backend::{
        entities::work_item::{WorkItemActiveModel, WorkItemModel},
        item_labels, projects,
        storage::utc_now,
    },
    shared::view_models::AgentReasoningEffort,
};

#[derive(Clone, Debug, Default)]
pub struct UpdateWorkItem {
    pub title: Option<String>,
    pub description: Option<String>,
    pub state: Option<String>,
    pub agent_model_override: Option<Option<String>>,
    pub agent_reasoning_effort_override: Option<Option<AgentReasoningEffort>>,
    pub expect_version: Option<i64>,
}

#[derive(Debug)]
pub(crate) struct WorkItemUpdatePlan {
    field_updates: WorkItemFieldUpdates,
    state: Option<String>,
    expect_version: Option<i64>,
}

#[derive(Debug)]
struct WorkItemFieldUpdates {
    title: Option<String>,
    description: Option<String>,
    agent_model_override: Option<Option<String>>,
    agent_reasoning_effort_override: Option<Option<AgentReasoningEffort>>,
}

#[derive(Debug)]
pub(crate) struct AppliedWorkItemUpdate {
    pub(crate) active: WorkItemActiveModel,
    pub(crate) state: Option<String>,
    pub(crate) record_item_updated_event: bool,
}

impl WorkItemUpdatePlan {
    pub(crate) fn new(update: UpdateWorkItem) -> Result<Self> {
        let state = update
            .state
            .map(item_labels::normalize_state_value)
            .transpose()
            .context("invalid item state")?;
        let field_updates = WorkItemFieldUpdates {
            title: update.title,
            description: update.description,
            agent_model_override: update.agent_model_override,
            agent_reasoning_effort_override: update.agent_reasoning_effort_override,
        };
        if !field_updates.has_any_update() && state.is_none() {
            bail!("item update requires at least one field");
        }

        Ok(Self {
            field_updates,
            state,
            expect_version: update.expect_version,
        })
    }

    pub(crate) fn expect_version(&self) -> Option<i64> {
        self.expect_version
    }

    pub(crate) fn apply_to(self, existing: WorkItemModel) -> Result<AppliedWorkItemUpdate> {
        let state = self.state;
        let record_item_updated_event = self.field_updates.has_any_update();
        let field_update = self.field_updates.apply_to(existing)?;

        Ok(AppliedWorkItemUpdate {
            active: field_update,
            state,
            record_item_updated_event,
        })
    }
}

impl WorkItemFieldUpdates {
    fn has_any_update(&self) -> bool {
        self.title.is_some()
            || self.description.is_some()
            || self.agent_model_override.is_some()
            || self.agent_reasoning_effort_override.is_some()
    }

    fn has_text_update(&self) -> bool {
        self.title.is_some() || self.description.is_some()
    }

    fn apply_to(self, existing: WorkItemModel) -> Result<WorkItemActiveModel> {
        let has_text_update = self.has_text_update();
        let Self {
            title,
            description,
            agent_model_override,
            agent_reasoning_effort_override,
        } = self;

        let title = title.unwrap_or_else(|| existing.title.clone());
        let description = description.unwrap_or_else(|| existing.description.clone());
        if has_text_update {
            validate_item_text(&title, &description)?;
        }

        let version = existing.version;
        let mut active: WorkItemActiveModel = existing.into();
        active.title = Set(title);
        active.description = Set(description);
        if let Some(agent_model_override) = agent_model_override {
            active.agent_model_override = Set(projects::normalize_optional(agent_model_override));
        }
        if let Some(agent_reasoning_effort_override) = agent_reasoning_effort_override {
            active.agent_reasoning_effort_override =
                Set(agent_reasoning_effort_override.map(|effort| effort.as_storage().to_owned()));
        }
        active.version = Set(version + 1);
        active.updated_at = Set(utc_now());

        Ok(active)
    }
}

pub(crate) fn validate_item_text(title: &str, description: &str) -> Result<()> {
    if title.trim().is_empty() {
        bail!("item title cannot be empty");
    }
    if description.trim().is_empty() {
        bail!("item description cannot be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::ActiveValue::Set;

    use super::*;

    fn work_item() -> WorkItemModel {
        WorkItemModel {
            id: 7,
            project_id: 3,
            title: "Existing title".to_owned(),
            description: "Existing description".to_owned(),
            claimed_by: None,
            claimed_at: None,
            claim_expires_at: None,
            finished_at: None,
            agent_model_override: Some("gpt-5.1".to_owned()),
            agent_reasoning_effort_override: Some("medium".to_owned()),
            version: 4,
            created_at: "2026-06-19T00:00:00Z".to_owned(),
            updated_at: "2026-06-19T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn empty_update_is_rejected_before_applying_to_model() {
        let err = WorkItemUpdatePlan::new(UpdateWorkItem::default()).unwrap_err();

        assert!(err.to_string().contains("requires at least one field"));
    }

    #[test]
    fn state_update_is_normalized_without_recording_item_field_event() {
        let plan = WorkItemUpdatePlan::new(UpdateWorkItem {
            state: Some(" review ".to_owned()),
            expect_version: Some(4),
            ..UpdateWorkItem::default()
        })
        .unwrap();

        assert_eq!(plan.expect_version(), Some(4));

        let applied = plan.apply_to(work_item()).unwrap();

        assert_eq!(applied.state.as_deref(), Some("review"));
        assert!(!applied.record_item_updated_event);
        assert_eq!(applied.active.version, Set(5));
    }

    #[test]
    fn model_override_clear_counts_as_item_field_update() {
        let plan = WorkItemUpdatePlan::new(UpdateWorkItem {
            agent_model_override: Some(None),
            ..UpdateWorkItem::default()
        })
        .unwrap();

        let applied = plan.apply_to(work_item()).unwrap();

        assert!(applied.record_item_updated_event);
        assert_eq!(applied.active.agent_model_override, Set(None));
        assert_eq!(applied.active.version, Set(5));
    }

    #[test]
    fn text_update_validates_effective_title_and_description() {
        let plan = WorkItemUpdatePlan::new(UpdateWorkItem {
            title: Some("  ".to_owned()),
            ..UpdateWorkItem::default()
        })
        .unwrap();

        let err = plan.apply_to(work_item()).unwrap_err();

        assert!(err.to_string().contains("item title cannot be empty"));
    }
}
