use std::{error::Error, fmt, str::FromStr};

use crudkit_core::condition::{
    Condition, ConditionClause, ConditionClauseValue, ConditionElement, Operator,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    ProjectListChanged {
        sequence: u64,
        timestamp: String,
    },
    ProjectChanged {
        sequence: u64,
        timestamp: String,
        project: String,
    },
    SystemPromptChanged {
        sequence: u64,
        timestamp: String,
        project: String,
    },
    WorkItemChanged {
        sequence: u64,
        timestamp: String,
        project: String,
        item_id: i64,
    },
    CommentChanged {
        sequence: u64,
        timestamp: String,
        project: String,
        item_id: i64,
    },
    MemoryChanged {
        sequence: u64,
        timestamp: String,
        project: String,
    },
    SwimLaneChanged {
        sequence: u64,
        timestamp: String,
        project: String,
    },
    WorkItemStateChanged {
        sequence: u64,
        timestamp: String,
        project: String,
    },
    AgentToolChanged {
        sequence: u64,
        timestamp: String,
    },
    AutomationChanged {
        sequence: u64,
        timestamp: String,
        project: String,
    },
    AgentRunChanged {
        sequence: u64,
        timestamp: String,
        project: String,
        run_id: i64,
        item_id: Option<i64>,
    },
    AgentOutputChanged {
        sequence: u64,
        timestamp: String,
        project: String,
        run_id: i64,
        item_id: Option<i64>,
    },
    CodexStatusChanged {
        sequence: u64,
        timestamp: String,
    },
}

#[derive(Debug, Clone)]
pub struct ParseEnumError(&'static str);

impl fmt::Display for ParseEnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Error for ParseEnumError {}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectView {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    pub path: Option<String>,
    pub path_exists: bool,
    pub path_checked_at: Option<String>,
    pub git_status: Option<ProjectGitStatusView>,
    pub system_prompt: String,
    pub memory: String,
    pub workspace_mode: WorkspaceMode,
    pub max_code_edit_agents: i64,
    pub allow_refinement_agents_during_editing: bool,
    pub create_pr: bool,
    pub auto_commit: bool,
    pub commit_standard: String,
    pub revert_strategy: RevertStrategy,
    pub stale_claim_minutes: i64,
    pub worktree_cleanup_policy: WorktreeCleanupPolicy,
    pub default_agent_tool: AgentToolName,
    pub default_agent_model: Option<String>,
    pub default_agent_reasoning_effort: Option<AgentReasoningEffort>,
    pub agent_sandbox_mode: AgentSandboxMode,
    pub agent_extra_writable_roots: Vec<String>,
    pub agent_git_command_policy: AgentGitCommandPolicy,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectGitStatusView {
    pub is_repository: bool,
    pub branch: Option<String>,
    pub added_lines: u64,
    pub deleted_lines: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkspaceEditorView {
    pub target: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectMemoryView {
    pub project_id: i64,
    pub project_name: String,
    pub memory: String,
    pub last_event: Option<ProjectMemoryEventView>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectMemoryEventView {
    pub id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub operation: String,
    pub memory: String,
    pub actor_type: Option<String>,
    pub actor_id: Option<String>,
    pub agent_run_id: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectMemoryUpdateView {
    pub project: ProjectView,
    pub event: ProjectMemoryEventView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectMemoryCompactionView {
    pub project_id: i64,
    pub project_name: String,
    pub deleted_events: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectSystemPromptView {
    pub project_id: i64,
    pub project_name: String,
    pub system_prompt: String,
    pub last_event: Option<ProjectSystemPromptEventView>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectSystemPromptEventView {
    pub id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub operation: String,
    pub system_prompt: String,
    pub actor_type: Option<String>,
    pub actor_id: Option<String>,
    pub agent_run_id: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectSystemPromptUpdateView {
    pub project: ProjectView,
    pub event: ProjectSystemPromptEventView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectSystemPromptCompactionView {
    pub project_id: i64,
    pub project_name: String,
    pub deleted_events: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolName {
    Codex,
}

impl AgentToolName {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Codex => "codex",
        }
    }

    pub fn all() -> [Self; 1] {
        [Self::Codex]
    }
}

impl fmt::Display for AgentToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AgentToolName {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "codex" => Ok(Self::Codex),
            _ => Err(ParseEnumError("agent tool must be codex")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexAgentModel {
    Gpt55,
    Gpt54,
    Gpt54Mini,
    Gpt53CodexSpark,
}

impl CodexAgentModel {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Gpt55 => "gpt-5.5",
            Self::Gpt54 => "gpt-5.4",
            Self::Gpt54Mini => "gpt-5.4-mini",
            Self::Gpt53CodexSpark => "gpt-5.3-codex-spark",
        }
    }

    pub fn all() -> [Self; 4] {
        [
            Self::Gpt55,
            Self::Gpt54,
            Self::Gpt54Mini,
            Self::Gpt53CodexSpark,
        ]
    }

    pub fn newest() -> Self {
        Self::Gpt55
    }

    pub fn is_available_model(value: &str) -> bool {
        value.parse::<Self>().is_ok()
    }

    pub fn allowed_values() -> String {
        Self::all()
            .iter()
            .map(|model| model.as_storage())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl fmt::Display for CodexAgentModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for CodexAgentModel {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "gpt-5.5" => Ok(Self::Gpt55),
            "gpt-5.4" => Ok(Self::Gpt54),
            "gpt-5.4-mini" => Ok(Self::Gpt54Mini),
            "gpt-5.3-codex-spark" => Ok(Self::Gpt53CodexSpark),
            _ => Err(ParseEnumError(
                "codex agent model must be one of: gpt-5.5, gpt-5.4, gpt-5.4-mini, gpt-5.3-codex-spark",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentToolView {
    pub id: i64,
    pub tool_name: AgentToolName,
    pub executable_path: Option<String>,
    pub discovered_path: Option<String>,
    pub effective_path: Option<String>,
    pub last_discovered_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CodexAppServerStatusView {
    pub available: bool,
    pub usable: bool,
    pub message: String,
    pub install_prompt: String,
    pub auth_setup: Option<CodexAuthSetupView>,
    pub checked_at: String,
    pub binary_path: Option<String>,
    pub requires_openai_auth: Option<bool>,
    pub signed_in: bool,
    pub auth_method: Option<String>,
    pub account_label: Option<String>,
    pub plan_type: Option<String>,
    pub payment_model: Option<String>,
    pub preconditions: Vec<CodexPreconditionView>,
    pub rate_limits: Vec<CodexRateLimitView>,
    pub usage_summary: Option<CodexUsageSummaryView>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CodexAuthSetupView {
    pub codex_home_path: String,
    pub codex_config_path: String,
    pub login_command: String,
    pub refresh_instruction: String,
    pub api_key_instruction: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CodexPreconditionView {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CodexRateLimitView {
    pub label: String,
    pub plan_type: Option<String>,
    pub primary_used_percent: Option<i64>,
    pub primary_window_minutes: Option<i64>,
    pub primary_resets_at: Option<String>,
    pub secondary_used_percent: Option<i64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<String>,
    pub individual_used: Option<String>,
    pub individual_limit: Option<String>,
    pub individual_remaining_percent: Option<i64>,
    pub individual_resets_at: Option<String>,
    pub credits_balance: Option<String>,
    pub credits_has_credits: Option<bool>,
    pub credits_unlimited: Option<bool>,
    pub reached_type: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CodexUsageSummaryView {
    pub lifetime_tokens: Option<i64>,
    pub peak_daily_tokens: Option<i64>,
    pub current_streak_days: Option<i64>,
    pub longest_streak_days: Option<i64>,
    pub longest_running_turn_seconds: Option<i64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    CurrentBranch,
    GitWorktree,
    GitBranch,
}

impl WorkspaceMode {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::CurrentBranch => "current_branch",
            Self::GitWorktree => "git_worktree",
            Self::GitBranch => "git_branch",
        }
    }
}

impl fmt::Display for WorkspaceMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for WorkspaceMode {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "current_branch" => Ok(Self::CurrentBranch),
            "git_worktree" => Ok(Self::GitWorktree),
            "git_branch" => Ok(Self::GitBranch),
            _ => Err(ParseEnumError(
                "workspace mode must be one of: current_branch, git_worktree, git_branch",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentGitHardResetPolicy {
    Never,
    #[default]
    IsolatedWorkspaces,
}

impl AgentGitHardResetPolicy {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::IsolatedWorkspaces => "isolated_workspaces",
        }
    }
}

impl fmt::Display for AgentGitHardResetPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AgentGitHardResetPolicy {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "never" => Ok(Self::Never),
            "isolated" | "isolated_workspace" | "isolated_workspaces" => {
                Ok(Self::IsolatedWorkspaces)
            }
            _ => Err(ParseEnumError(
                "agent git hard-reset policy must be one of: never, isolated_workspaces",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentGitCommandPolicy {
    pub add: bool,
    pub commit: bool,
    pub push: bool,
    pub reset: bool,
    pub hard_reset: AgentGitHardResetPolicy,
}

impl AgentGitCommandPolicy {
    pub fn allows_hard_reset(&self, workspace_mode: WorkspaceMode) -> bool {
        self.reset
            && match self.hard_reset {
                AgentGitHardResetPolicy::Never => false,
                AgentGitHardResetPolicy::IsolatedWorkspaces => {
                    workspace_mode != WorkspaceMode::CurrentBranch
                }
            }
    }
}

impl Default for AgentGitCommandPolicy {
    fn default() -> Self {
        Self {
            add: true,
            commit: true,
            push: true,
            reset: true,
            hard_reset: AgentGitHardResetPolicy::IsolatedWorkspaces,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentGitRuntimePolicy {
    pub policy: AgentGitCommandPolicy,
    pub workspace_mode: WorkspaceMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeCleanupPolicy {
    Manual,
    AfterSuccess,
}

impl WorktreeCleanupPolicy {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AfterSuccess => "after_success",
        }
    }
}

impl fmt::Display for WorktreeCleanupPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for WorktreeCleanupPolicy {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "manual" => Ok(Self::Manual),
            "after_success" => Ok(Self::AfterSuccess),
            _ => Err(ParseEnumError(
                "worktree cleanup policy must be one of: manual, after_success",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevertStrategy {
    Manual,
    GitReset,
}

impl RevertStrategy {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::GitReset => "git_reset",
        }
    }
}

impl fmt::Display for RevertStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for RevertStrategy {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "manual" => Ok(Self::Manual),
            "git_reset" => Ok(Self::GitReset),
            _ => Err(ParseEnumError(
                "revert strategy must be one of: manual, git_reset",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectSettingsView {
    pub id: i64,
    pub project_id: i64,
    pub workspace_mode: WorkspaceMode,
    pub max_code_edit_agents: i64,
    pub allow_refinement_agents_during_editing: bool,
    pub create_pr: bool,
    pub auto_commit: bool,
    pub commit_standard: String,
    pub revert_strategy: RevertStrategy,
    pub stale_claim_minutes: i64,
    pub worktree_cleanup_policy: WorktreeCleanupPolicy,
    pub default_agent_tool: AgentToolName,
    pub default_agent_model: Option<String>,
    pub default_agent_reasoning_effort: Option<AgentReasoningEffort>,
    pub agent_sandbox_mode: AgentSandboxMode,
    pub agent_extra_writable_roots: Vec<String>,
    pub agent_git_command_policy: AgentGitCommandPolicy,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSandboxMode {
    WorkspaceWrite,
    DangerFullAccess,
}

impl AgentSandboxMode {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::WorkspaceWrite => "workspace_write",
            Self::DangerFullAccess => "danger_full_access",
        }
    }
}

impl fmt::Display for AgentSandboxMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AgentSandboxMode {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "workspace_write" | "workspacewrite" => Ok(Self::WorkspaceWrite),
            "danger_full_access" | "dangerfullaccess" => Ok(Self::DangerFullAccess),
            _ => Err(ParseEnumError(
                "agent sandbox mode must be one of: workspace_write, danger_full_access",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

impl AgentReasoningEffort {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    pub fn all() -> [Self; 6] {
        [
            Self::None,
            Self::Minimal,
            Self::Low,
            Self::Medium,
            Self::High,
            Self::XHigh,
        ]
    }

    pub fn highest() -> Self {
        Self::XHigh
    }
}

impl fmt::Display for AgentReasoningEffort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AgentReasoningEffort {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" | "x_high" => Ok(Self::XHigh),
            _ => Err(ParseEnumError(
                "agent reasoning effort must be one of: none, minimal, low, medium, high, xhigh",
            )),
        }
    }
}

pub const STATE_LABEL_KEY: &str = "state";
pub const DEFAULT_STATE_LABEL: &str = "open";
pub const CLAIMED_STATE_LABEL: &str = "in_progress";
pub const FINISHED_STATE_LABEL: &str = "done";
pub const CLAIMED_FROM_STATE_LABEL_KEY: &str = "patchbay:claimed-from-state";
pub const AUTOMATION_BLOCKED_LABEL_KEY: &str = "patchbay:automation-blocked";
pub const NEEDS_REFINEMENT_LABEL_KEY: &str = "needs-refinement";
pub const NEEDS_VERIFICATION_LABEL_KEY: &str = "needs-verification";

pub fn default_automation_work_item_selector() -> Condition {
    Condition::All(vec![
        ConditionElement::Clause(ConditionClause {
            column_name: STATE_LABEL_KEY.to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::String(DEFAULT_STATE_LABEL.to_owned()),
        }),
        ConditionElement::Clause(ConditionClause {
            column_name: NEEDS_REFINEMENT_LABEL_KEY.to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::Bool(false),
        }),
        ConditionElement::Clause(ConditionClause {
            column_name: NEEDS_VERIFICATION_LABEL_KEY.to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::Bool(false),
        }),
    ])
}

pub fn needs_refinement_automation_work_item_selector() -> Condition {
    label_presence_selector(NEEDS_REFINEMENT_LABEL_KEY)
}

pub fn needs_verification_automation_work_item_selector() -> Condition {
    label_presence_selector(NEEDS_VERIFICATION_LABEL_KEY)
}

fn label_presence_selector(label_key: &str) -> Condition {
    Condition::All(vec![ConditionElement::Clause(ConditionClause {
        column_name: label_key.to_owned(),
        operator: Operator::Equal,
        value: ConditionClauseValue::Bool(true),
    })])
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkItemView {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub description: String,
    pub state: Option<String>,
    pub labels: Vec<WorkItemLabelView>,
    pub version: i64,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub claim_expires_at: Option<String>,
    pub finished_at: Option<String>,
    pub agent_model_override: Option<String>,
    pub agent_reasoning_effort_override: Option<AgentReasoningEffort>,
    pub created_at: String,
    pub updated_at: String,
    pub comment_count: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkItemLabelView {
    pub id: i64,
    pub project_id: i64,
    pub work_item_id: i64,
    pub key: String,
    pub value: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectLabelView {
    pub key: String,
    pub value: Option<String>,
    pub usage_count: i64,
    pub last_used_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SwimLaneView {
    pub id: i64,
    pub project_id: i64,
    pub identifier: String,
    pub name: String,
    pub position: i64,
    pub filter: Condition,
    pub item_order: String,
    pub can_create_items: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkItemStateView {
    pub id: i64,
    pub project_id: i64,
    pub identifier: String,
    pub name: String,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkItemEventView {
    pub id: i64,
    pub project_id: i64,
    pub work_item_id: Option<i64>,
    pub event_type: String,
    pub body: String,
    pub actor_type: Option<String>,
    pub actor_id: Option<String>,
    pub agent_run_id: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecoveredClaimView {
    pub item_id: i64,
    pub agent_id: String,
    pub claimed_at: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorType {
    User,
    Agent,
    System,
}

impl AuthorType {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::System => "system",
        }
    }
}

impl fmt::Display for AuthorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AuthorType {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "user" => Ok(Self::User),
            "agent" => Ok(Self::Agent),
            "system" => Ok(Self::System),
            _ => Err(ParseEnumError(
                "author type must be one of: user, agent, system",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommentView {
    pub id: i64,
    pub work_item_id: i64,
    pub author_type: AuthorType,
    pub author_name: Option<String>,
    pub body: String,
    pub created_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl AgentRunStatus {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for AgentRunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AgentRunStatus {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseEnumError(
                "agent run status must be one of: running, completed, failed, cancelled",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCommitOutcome {
    NotEvaluated,
    NotRequired,
    Committed,
    SkippedNoChanges,
    SkippedNoGitRepo,
    MissingRequired,
    Unknown,
}

impl AgentCommitOutcome {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::NotEvaluated => "not_evaluated",
            Self::NotRequired => "not_required",
            Self::Committed => "committed",
            Self::SkippedNoChanges => "skipped_no_changes",
            Self::SkippedNoGitRepo => "skipped_no_git_repo",
            Self::MissingRequired => "missing_required",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for AgentCommitOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AgentCommitOutcome {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "not_evaluated" => Ok(Self::NotEvaluated),
            "not_required" => Ok(Self::NotRequired),
            "committed" => Ok(Self::Committed),
            "skipped_no_changes" => Ok(Self::SkippedNoChanges),
            "skipped_no_git_repo" => Ok(Self::SkippedNoGitRepo),
            "missing_required" => Ok(Self::MissingRequired),
            "unknown" => Ok(Self::Unknown),
            _ => Err(ParseEnumError(
                "commit outcome must be one of: not_evaluated, not_required, committed, skipped_no_changes, skipped_no_git_repo, missing_required, unknown",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentRunView {
    pub id: i64,
    pub project_id: i64,
    pub work_item_id: Option<i64>,
    pub memory_event_id: Option<i64>,
    pub trigger_id: Option<i64>,
    pub trigger_name: Option<String>,
    pub tool_name: AgentToolName,
    pub status: AgentRunStatus,
    pub command: String,
    pub working_dir: String,
    pub worktree_path: Option<String>,
    pub branch_name: Option<String>,
    pub process_id: Option<i64>,
    pub exit_code: Option<i64>,
    pub log_path: Option<String>,
    pub prompt_path: Option<String>,
    pub agent_model: Option<String>,
    pub agent_reasoning_effort: Option<AgentReasoningEffort>,
    pub token_usage: Option<AgentRunTokenUsageView>,
    pub commit_required: bool,
    pub commit_outcome: AgentCommitOutcome,
    pub commit_shas: Vec<String>,
    pub pr_requested: bool,
    pub pr_url: Option<String>,
    pub cleanup_status: String,
    pub worktree_cleaned_at: Option<String>,
    pub result_summary: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunTokenUsageView {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunLogView {
    pub run: AgentRunView,
    #[serde(default)]
    pub active: bool,
    pub memory_event: Option<ProjectMemoryEventRefView>,
    pub prompt: Option<String>,
    pub output: Vec<AgentRunOutputPiece>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentRunOutputLog {
    pub schema_version: u32,
    pub pieces: Vec<AgentRunOutputPiece>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentRunOutputPiece {
    pub sequence: u64,
    pub timestamp: String,
    pub kind: AgentRunOutputKind,
    pub source: String,
    pub item_id: Option<String>,
    pub title: String,
    pub body: String,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunOutputKind {
    System,
    ModelMessage,
    Reasoning,
    ToolCall,
    FileChange,
    Error,
    Legacy,
}

impl AgentRunOutputKind {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ModelMessage => "model_message",
            Self::Reasoning => "reasoning",
            Self::ToolCall => "tool_call",
            Self::FileChange => "file_change",
            Self::Error => "error",
            Self::Legacy => "legacy",
        }
    }
}

impl fmt::Display for AgentRunOutputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectMemoryEventRefView {
    pub event_id: i64,
    pub available: bool,
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AutomationStatusView {
    pub project: String,
    pub settings: ProjectSettingsView,
    pub running_runs: i64,
    pub recent_runs: Vec<AgentRunView>,
    pub tools: Vec<AgentToolView>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationActivation {
    Manual,
    WorkItem,
    Cron,
    WorkItemCreated,
}

impl AutomationActivation {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::WorkItem => "work_item",
            Self::Cron => "cron",
            Self::WorkItemCreated => "work_item_created",
        }
    }
}

impl fmt::Display for AutomationActivation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AutomationActivation {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "manual" => Ok(Self::Manual),
            "work_item" | "started" | "start" => Ok(Self::WorkItem),
            "cron" => Ok(Self::Cron),
            "work_item_created" => Ok(Self::WorkItemCreated),
            _ => Err(ParseEnumError(
                "automation activation must be one of: manual, work_item, cron, work_item_created",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationEffect {
    ProduceWork,
    ConsumeWork,
}

impl AutomationEffect {
    pub fn as_storage(self) -> &'static str {
        match self {
            Self::ProduceWork => "produce_work",
            Self::ConsumeWork => "consume_work",
        }
    }
}

impl fmt::Display for AutomationEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_storage())
    }
}

impl FromStr for AutomationEffect {
    type Err = ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().replace('-', "_").as_str() {
            "produce_work" | "produce" | "producer" => Ok(Self::ProduceWork),
            "consume_work" | "consume" | "consumer" => Ok(Self::ConsumeWork),
            _ => Err(ParseEnumError(
                "automation effect must be one of: produce_work, consume_work",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AutomationTriggerView {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub enabled: bool,
    #[serde(alias = "trigger_kind")]
    pub activation: AutomationActivation,
    pub effect: AutomationEffect,
    pub schedule: String,
    pub tool_name: AgentToolName,
    pub prompt: String,
    pub work_item_selector: Option<Condition>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TriggerRunOutcome {
    pub trigger_id: i64,
    pub trigger_name: String,
    pub work_item_id: Option<i64>,
    pub work_item: Option<WorkItemView>,
    pub run: Option<AgentRunView>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProcessSessionView {
    pub run_id: i64,
    pub project_name: String,
    pub tool_name: String,
    pub command: String,
    pub working_dir: String,
    pub process_id: Option<i64>,
    pub output: Vec<AgentRunOutputPiece>,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ApiError {
    pub error: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateWorkItemRequest {
    pub title: String,
    pub description: String,
    pub state: Option<String>,
    pub agent_model_override: Option<String>,
    pub agent_reasoning_effort_override: Option<AgentReasoningEffort>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UpdateWorkItemRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub state: Option<String>,
    pub agent_model_override: Option<Option<String>>,
    pub agent_reasoning_effort_override: Option<Option<AgentReasoningEffort>>,
    pub expect_version: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClaimWorkItemRequest {
    pub agent_id: String,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClaimWorkItemResponse {
    pub item: Option<WorkItemView>,
}

impl ClaimWorkItemResponse {
    pub fn claimed(&self) -> bool {
        self.item.is_some()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateWorkItemLabelRequest {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UpdateWorkItemLabelRequest {
    pub key: Option<String>,
    pub value: Option<Option<String>>,
    pub expect_version: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeleteWorkItemLabelResponse {
    pub deleted: bool,
    pub label_id: i64,
    pub work_item: WorkItemView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProgressWorkItemRequest {
    pub agent_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FinishWorkItemRequest {
    pub agent_id: String,
    pub report: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReleaseWorkItemRequest {
    pub agent_id: String,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateProjectMemoryRequest {
    pub agent_id: String,
    pub agent_run_id: Option<i64>,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AddCommentRequest {
    pub author_type: AuthorType,
    pub author_name: Option<String>,
    pub body: String,
}
