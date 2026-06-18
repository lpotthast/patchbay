pub mod automation_trigger {
    use crudkit_leptos::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, CkResource, Serialize, Deserialize)]
    #[ck_resource(resource_name = "automation_triggers")]
    #[ck_field(model = ModelType::Update)]
    pub struct AutomationTrigger {
        pub id: i64,
        pub name: String,
        pub enabled: bool,
        pub activation: String,
        pub effect: String,
        pub schedule: String,
        pub tool_name: String,
        pub mutability: String,
        pub prompt: String,
        pub work_item_selector: Option<String>,
        pub priority: i64,
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Create)]
    pub struct CreateAutomationTrigger {
        pub project_id: i64,
        pub name: String,
        pub enabled: bool,
        pub activation: String,
        pub effect: String,
        pub schedule: String,
        pub tool_name: String,
        pub mutability: String,
        pub prompt: String,
        pub work_item_selector: Option<String>,
        pub priority: i64,
    }

    impl Default for CreateAutomationTrigger {
        fn default() -> Self {
            Self {
                project_id: 0,
                name: String::new(),
                enabled: true,
                activation: "work_item".to_owned(),
                effect: "consume_work".to_owned(),
                schedule: "@every 10s".to_owned(),
                tool_name: "codex".to_owned(),
                mutability: "mutating".to_owned(),
                prompt: String::new(),
                work_item_selector: Some(
                    r#"{"All":[{"column_name":"state","operator":"=","value":{"String":"open"}},{"column_name":"needs-refinement","operator":"=","value":{"Bool":false}},{"column_name":"needs-verification","operator":"=","value":{"Bool":false}}]}"#
                        .to_owned(),
                ),
                priority: 0,
            }
        }
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Read)]
    pub struct ReadAutomationTrigger {
        pub id: i64,
        pub project_id: i64,
        pub name: String,
        pub enabled: bool,
        pub activation: String,
        pub effect: String,
        pub schedule: String,
        pub tool_name: String,
        pub mutability: String,
        pub prompt: String,
        pub work_item_selector: Option<String>,
        pub priority: i64,
        pub evaluation_count: i64,
        pub pending_evaluation_count: i64,
        pub last_evaluation_queued_at: Option<String>,
        pub last_evaluated_at: Option<String>,
        pub next_evaluation_at: Option<String>,
        pub last_event_id: Option<i64>,
        pub created_at: String,
        pub updated_at: String,
    }

    impl From<ReadAutomationTrigger> for AutomationTrigger {
        fn from(read: ReadAutomationTrigger) -> Self {
            Self {
                id: read.id,
                name: read.name,
                enabled: read.enabled,
                activation: read.activation,
                effect: read.effect,
                schedule: read.schedule,
                tool_name: read.tool_name,
                mutability: read.mutability,
                prompt: read.prompt,
                work_item_selector: read.work_item_selector,
                priority: read.priority,
            }
        }
    }

    impl ErasedIdentifiable for CreateAutomationTrigger {
        fn id(&self) -> SerializableId {
            panic!("create models are not identifiable")
        }
    }
}

pub mod agent_tool {
    use crudkit_leptos::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, CkResource, Serialize, Deserialize)]
    #[ck_resource(resource_name = "agent_tools")]
    #[ck_field(model = ModelType::Update)]
    pub struct AgentTool {
        pub id: i64,
        pub executable_path: Option<String>,
    }

    #[derive(Clone, PartialEq, Eq, Debug, Default, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Create)]
    pub struct CreateAgentTool {
        pub tool_name: String,
        pub executable_path: Option<String>,
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Read)]
    pub struct ReadAgentTool {
        pub id: i64,
        pub tool_name: String,
        pub executable_path: Option<String>,
        pub discovered_path: Option<String>,
        pub last_discovered_at: Option<String>,
        pub created_at: String,
        pub updated_at: String,
    }

    impl From<ReadAgentTool> for AgentTool {
        fn from(read: ReadAgentTool) -> Self {
            Self {
                id: read.id,
                executable_path: read.executable_path,
            }
        }
    }

    impl ErasedIdentifiable for CreateAgentTool {
        fn id(&self) -> SerializableId {
            panic!("create models are not identifiable")
        }
    }
}

pub mod project {
    use crate::shared::view_models::{AgentReasoningEffort, CodexAgentModel};

    use crudkit_leptos::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, CkResource, Serialize, Deserialize)]
    #[ck_resource(resource_name = "projects")]
    #[ck_field(model = ModelType::Update)]
    pub struct Project {
        pub id: i64,
        pub display_name: String,
        pub path: String,
        pub memory: String,
        pub workspace_mode: String,
        pub max_code_edit_agents: i64,
        pub max_read_only_agents: i64,
        pub create_pr: bool,
        pub auto_commit: bool,
        pub commit_standard: String,
        pub revert_strategy: String,
        pub stale_claim_minutes: i64,
        pub worktree_cleanup_policy: String,
        pub default_agent_tool: String,
        pub default_agent_model: Option<String>,
        pub default_agent_reasoning_effort: Option<String>,
        pub agent_sandbox_mode: String,
        pub agent_extra_writable_roots: String,
        pub agent_git_command_policy: String,
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Create)]
    pub struct CreateProject {
        pub name: String,
        pub display_name: String,
        pub path: String,
        pub default_agent_model: Option<String>,
        pub default_agent_reasoning_effort: Option<String>,
        pub memory: String,
    }

    impl Default for CreateProject {
        fn default() -> Self {
            Self {
                name: String::new(),
                display_name: String::new(),
                path: String::new(),
                default_agent_model: Some(CodexAgentModel::newest().as_storage().to_owned()),
                default_agent_reasoning_effort: Some(
                    AgentReasoningEffort::highest().as_storage().to_owned(),
                ),
                memory: String::new(),
            }
        }
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Read)]
    pub struct ReadProject {
        pub id: i64,
        pub name: String,
        pub display_name: String,
        pub path: Option<String>,
        pub path_exists: bool,
        pub path_checked_at: Option<String>,
        pub system_prompt: String,
        pub memory: String,
        pub workspace_mode: String,
        pub max_code_edit_agents: i64,
        pub max_read_only_agents: i64,
        pub create_pr: bool,
        pub auto_commit: bool,
        pub commit_standard: String,
        pub revert_strategy: String,
        pub stale_claim_minutes: i64,
        pub worktree_cleanup_policy: String,
        pub default_agent_tool: String,
        pub default_agent_model: Option<String>,
        pub default_agent_reasoning_effort: Option<String>,
        pub agent_sandbox_mode: String,
        pub agent_extra_writable_roots: String,
        pub agent_git_command_policy: String,
        pub created_at: String,
        pub updated_at: String,
    }

    impl From<ReadProject> for Project {
        fn from(read: ReadProject) -> Self {
            Self {
                id: read.id,
                display_name: read.display_name,
                path: read.path.unwrap_or_default(),
                memory: read.memory,
                workspace_mode: read.workspace_mode,
                max_code_edit_agents: read.max_code_edit_agents,
                max_read_only_agents: read.max_read_only_agents,
                create_pr: read.create_pr,
                auto_commit: read.auto_commit,
                commit_standard: read.commit_standard,
                revert_strategy: read.revert_strategy,
                stale_claim_minutes: read.stale_claim_minutes,
                worktree_cleanup_policy: read.worktree_cleanup_policy,
                default_agent_tool: read.default_agent_tool,
                default_agent_model: read.default_agent_model,
                default_agent_reasoning_effort: read.default_agent_reasoning_effort,
                agent_sandbox_mode: read.agent_sandbox_mode,
                agent_extra_writable_roots: read.agent_extra_writable_roots,
                agent_git_command_policy: read.agent_git_command_policy,
            }
        }
    }

    impl ErasedIdentifiable for CreateProject {
        fn id(&self) -> SerializableId {
            panic!("create models are not identifiable")
        }
    }
}

pub mod work_item {
    use crudkit_leptos::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, CkResource, Serialize, Deserialize)]
    #[ck_resource(resource_name = "work_items")]
    #[ck_field(model = ModelType::Update)]
    pub struct WorkItem {
        pub id: i64,
        pub title: String,
        pub description: String,
        pub agent_model_override: Option<String>,
        pub agent_reasoning_effort_override: Option<String>,
    }

    #[derive(Clone, PartialEq, Eq, Debug, Default, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Create)]
    pub struct CreateWorkItem {
        pub project_id: i64,
        pub title: String,
        pub description: String,
        pub agent_model_override: Option<String>,
        pub agent_reasoning_effort_override: Option<String>,
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Read)]
    pub struct ReadWorkItem {
        pub id: i64,
        pub project_id: i64,
        pub title: String,
        pub description: String,
        pub claimed_by: Option<String>,
        pub claimed_at: Option<String>,
        pub claim_expires_at: Option<String>,
        pub finished_at: Option<String>,
        pub agent_model_override: Option<String>,
        pub agent_reasoning_effort_override: Option<String>,
        pub version: i64,
        pub created_at: String,
        pub updated_at: String,
        pub state_label: Option<String>,
        pub has_validation_errors: bool,
    }

    impl From<ReadWorkItem> for WorkItem {
        fn from(read: ReadWorkItem) -> Self {
            Self {
                id: read.id,
                title: read.title,
                description: read.description,
                agent_model_override: read.agent_model_override,
                agent_reasoning_effort_override: read.agent_reasoning_effort_override,
            }
        }
    }

    impl ErasedIdentifiable for CreateWorkItem {
        fn id(&self) -> SerializableId {
            panic!("create models are not identifiable")
        }
    }
}

pub mod swim_lane {
    use crudkit_leptos::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, CkResource, Serialize, Deserialize)]
    #[ck_resource(resource_name = "swim_lanes")]
    #[ck_field(model = ModelType::Update)]
    pub struct SwimLane {
        pub id: i64,
        pub identifier: String,
        pub name: String,
        pub position: i64,
        pub filter: String,
        pub item_order: String,
        pub can_create_items: bool,
    }

    #[derive(Clone, PartialEq, Eq, Debug, Default, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Create)]
    pub struct CreateSwimLane {
        pub project_id: i64,
        pub identifier: String,
        pub name: String,
        pub position: i64,
        pub filter: String,
        pub item_order: String,
        pub can_create_items: bool,
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Read)]
    pub struct ReadSwimLane {
        pub id: i64,
        pub project_id: i64,
        pub identifier: String,
        pub name: String,
        pub position: i64,
        pub filter: String,
        pub item_order: String,
        pub can_create_items: bool,
        pub created_at: String,
        pub updated_at: String,
        pub has_validation_errors: bool,
    }

    impl From<ReadSwimLane> for SwimLane {
        fn from(read: ReadSwimLane) -> Self {
            Self {
                id: read.id,
                identifier: read.identifier,
                name: read.name,
                position: read.position,
                filter: read.filter,
                item_order: read.item_order,
                can_create_items: read.can_create_items,
            }
        }
    }

    impl ErasedIdentifiable for CreateSwimLane {
        fn id(&self) -> SerializableId {
            panic!("create models are not identifiable")
        }
    }
}

pub mod work_item_state {
    use crudkit_leptos::prelude::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, CkResource, Serialize, Deserialize)]
    #[ck_resource(resource_name = "work_item_states")]
    #[ck_field(model = ModelType::Update)]
    pub struct WorkItemState {
        pub id: i64,
        pub identifier: String,
        pub name: String,
        pub position: i64,
    }

    #[derive(Clone, PartialEq, Eq, Debug, Default, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Create)]
    pub struct CreateWorkItemState {
        pub project_id: i64,
        pub identifier: String,
        pub name: String,
        pub position: i64,
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkId, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Read)]
    pub struct ReadWorkItemState {
        pub id: i64,
        pub project_id: i64,
        pub identifier: String,
        pub name: String,
        pub position: i64,
        pub created_at: String,
        pub updated_at: String,
        pub has_validation_errors: bool,
    }

    impl From<ReadWorkItemState> for WorkItemState {
        fn from(read: ReadWorkItemState) -> Self {
            Self {
                id: read.id,
                identifier: read.identifier,
                name: read.name,
                position: read.position,
            }
        }
    }

    impl ErasedIdentifiable for CreateWorkItemState {
        fn id(&self) -> SerializableId {
            panic!("create models are not identifiable")
        }
    }
}
