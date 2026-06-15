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
        pub trigger_kind: String,
        pub schedule: Option<String>,
        pub mode: String,
        pub tool_name: String,
        pub prompt: String,
    }

    #[derive(Clone, PartialEq, Eq, Debug, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Create)]
    pub struct CreateAutomationTrigger {
        pub project_id: i64,
        pub name: String,
        pub enabled: bool,
        pub trigger_kind: String,
        pub schedule: Option<String>,
        pub mode: String,
        pub tool_name: String,
        pub prompt: String,
    }

    impl Default for CreateAutomationTrigger {
        fn default() -> Self {
            Self {
                project_id: 0,
                name: String::new(),
                enabled: true,
                trigger_kind: "work_item_created".to_owned(),
                schedule: None,
                mode: "refine".to_owned(),
                tool_name: "codex".to_owned(),
                prompt: String::new(),
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
        pub trigger_kind: String,
        pub schedule: Option<String>,
        pub mode: String,
        pub tool_name: String,
        pub prompt: String,
        pub last_run_at: Option<String>,
        pub next_run_at: Option<String>,
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
                trigger_kind: read.trigger_kind,
                schedule: read.schedule,
                mode: read.mode,
                tool_name: read.tool_name,
                prompt: read.prompt,
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
        pub allow_refinement_agents_during_editing: bool,
        pub create_pr: bool,
        pub stale_claim_minutes: i64,
        pub worktree_cleanup_policy: String,
        pub default_agent_tool: String,
        pub default_agent_model: Option<String>,
        pub default_agent_reasoning_effort: Option<String>,
    }

    #[derive(Clone, PartialEq, Eq, Debug, Default, CkField, Serialize, Deserialize)]
    #[ck_field(model = ModelType::Create)]
    pub struct CreateProject {
        pub name: String,
        pub display_name: String,
        pub path: String,
        pub default_agent_model: Option<String>,
        pub memory: String,
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
        pub allow_refinement_agents_during_editing: bool,
        pub create_pr: bool,
        pub stale_claim_minutes: i64,
        pub worktree_cleanup_policy: String,
        pub default_agent_tool: String,
        pub default_agent_model: Option<String>,
        pub default_agent_reasoning_effort: Option<String>,
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
                allow_refinement_agents_during_editing: read.allow_refinement_agents_during_editing,
                create_pr: read.create_pr,
                stale_claim_minutes: read.stale_claim_minutes,
                worktree_cleanup_policy: read.worktree_cleanup_policy,
                default_agent_tool: read.default_agent_tool,
                default_agent_model: read.default_agent_model,
                default_agent_reasoning_effort: read.default_agent_reasoning_effort,
            }
        }
    }

    impl ErasedIdentifiable for CreateProject {
        fn id(&self) -> SerializableId {
            panic!("create models are not identifiable")
        }
    }
}
