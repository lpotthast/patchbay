use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command as StdCommand,
    str::FromStr,
    sync::OnceLock,
    time::Duration,
};

use codex_app_server_sdk::{
    ApprovalMode, ClientError, ModelReasoningEffort as CodexReasoningEffort, SandboxMode,
    StreamedTurn, Thread, ThreadEvent, ThreadOptions, TurnOptions,
};
use crudkit_core::condition::Condition;
use git2::{
    Repository, StatusOptions, WorktreeAddOptions, WorktreePruneOptions, build::CheckoutBuilder,
};
use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use tokio::{sync::watch, time::timeout};

use crate::{
    backend::{
        agent_ids, agent_tools,
        automation_commit::{
            CommitOutcomeEvaluation, capture_commit_baseline, evaluate_commit_outcome_for_run,
        },
        automation_output::{
            OutputPieceDraft, new_output_piece, push_codex_output_piece, read_run_output,
            read_run_token_usage, thread_event_output_piece, update_response_candidates,
            write_run_output_log,
        },
        automation_prompt::{PromptContext, build_prompt},
        codex_app_server,
        entities::agent_run::{self, AgentRun, AgentRunActiveModel, AgentRunModel},
        events, item_claims, items, personalities,
        process_sessions::{ProcessSessionRegistry, ProcessSessionStart},
        projects,
        storage::{Store, utc_now},
    },
    shared::view_models::{
        AgentCommitOutcome, AgentGitHardResetPolicy, AgentGitRuntimePolicy, AgentReasoningEffort,
        AgentRunOutputKind, AgentRunOutputPiece, AgentRunStatus, AgentRunTokenUsageView,
        AgentRunView, AgentSandboxMode, AgentToolName, AutomationRunMutability,
        AutomationStatusView, DEFAULT_STATE_LABEL, FINISHED_STATE_LABEL, ProjectMemoryEventRefView,
        ProjectSettingsView, RecoveredClaimView, RunLogView, WorkItemView, WorkspaceMode,
        WorktreeCleanupPolicy,
    },
};

const AGENT_PROCESS_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);
const CODEX_STREAM_RECOVERY_MAX_ATTEMPTS: usize = 12;
static SERVER_API_URL: OnceLock<String> = OnceLock::new();
const CODEX_STREAM_RECOVERY_PROMPT: &str = "\
Patchbay recovered from a transient Codex app-server reconnect or transport interruption during \
this automation run. Continue from the existing thread context, current repository state, and \
current Patchbay item state. Do not repeat completed work; proceed to the final answer when the \
task is complete.";

#[derive(Clone, Debug)]
pub struct StartAutomation {
    pub tool: Option<AgentToolName>,
    pub work_item_id: Option<i64>,
    pub work_item_selector: Option<Condition>,
    pub extra_prompt: Option<String>,
    pub mutability: Option<AutomationRunMutability>,
    pub personality_id: Option<i64>,
    pub trigger: Option<AutomationTriggerOrigin>,
}

#[derive(Clone, Debug)]
pub struct AutomationTriggerOrigin {
    pub trigger_id: i64,
    pub trigger_name: String,
}

struct WorkspacePlan {
    working_dir: PathBuf,
    worktree_path: Option<PathBuf>,
    branch_name: Option<String>,
}

struct GitRuntimeFiles {
    shim_dir: PathBuf,
    policy_path: PathBuf,
}

struct LaunchDetails {
    work_item_id: Option<i64>,
    command: String,
    workspace: WorkspacePlan,
    prompt_path: Option<String>,
    log_path: Option<String>,
    memory_event_id: Option<i64>,
    agent_model: Option<String>,
    agent_reasoning_effort: Option<AgentReasoningEffort>,
    commit_required: bool,
    pr_requested: bool,
}

#[derive(Debug)]
struct AgentProcessOutput {
    process_id: Option<i64>,
    output: Vec<AgentRunOutputPiece>,
    final_response: String,
    token_usage: Option<AgentRunTokenUsageView>,
}

#[derive(Debug)]
enum CodexStreamStartError {
    Spawn(Report),
    Run(ClientError),
}

impl fmt::Display for CodexStreamStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(err) => write!(f, "{err}"),
            Self::Run(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for CodexStreamStartError {}

struct CodexStreamRecoveryContext<'a> {
    start: &'a AgentProcessStart,
    sessions: &'a Option<ProcessSessionRegistry>,
    env: &'a HashMap<String, String>,
    thread_options: &'a ThreadOptions,
    output: &'a mut Vec<AgentRunOutputPiece>,
}

struct AgentProcessStart {
    run_id: i64,
    project_name: String,
    tool_name: AgentToolName,
    codex_binary: PathBuf,
    codex_home: PathBuf,
    patchbay_binary: PathBuf,
    prompt_path: PathBuf,
    working_dir: PathBuf,
    git_shim_dir: PathBuf,
    git_policy_path: PathBuf,
    real_git_path: PathBuf,
    agent_id: String,
    claimed_item_id: Option<i64>,
    agent_model: Option<String>,
    agent_reasoning_effort: Option<AgentReasoningEffort>,
    agent_sandbox_mode: AgentSandboxMode,
    agent_extra_writable_roots: Vec<String>,
    mutability: AutomationRunMutability,
}

#[derive(Clone, Copy)]
enum ClaimReleaseReason {
    Completed,
    Failed,
    Cancelled,
}

struct ClaimReleaseContext<'a> {
    project_name: &'a str,
    run_id: i64,
    claimed_item: Option<&'a WorkItemView>,
    agent_id: &'a str,
    reason: ClaimReleaseReason,
    detail: Option<&'a str>,
    automation_disposition: item_claims::ReleaseAutomationDisposition,
}

#[derive(Debug)]
struct AutomationCancelled;

impl fmt::Display for AutomationCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("automation run was cancelled")
    }
}

impl std::error::Error for AutomationCancelled {}

fn is_automation_cancelled(err: &Report) -> bool {
    err.iter_reports().any(|report| {
        report
            .downcast_current_context::<AutomationCancelled>()
            .is_some()
    })
}

fn cancellation_requested(cancellation: &Option<watch::Receiver<bool>>) -> bool {
    cancellation
        .as_ref()
        .is_some_and(|cancellation| *cancellation.borrow())
}

struct StartedAutomationRun {
    project_name: String,
    project: crate::shared::view_models::ProjectView,
    settings: ProjectSettingsView,
    start: StartAutomation,
    tool: AgentToolName,
    run: AgentRunModel,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RunningRunCounts {
    pub mutating: i64,
    pub read_only: i64,
}

impl RunningRunCounts {
    fn total(self) -> i64 {
        self.mutating.saturating_add(self.read_only)
    }

    fn for_mutability(self, mutability: AutomationRunMutability) -> i64 {
        match mutability {
            AutomationRunMutability::Mutating => self.mutating,
            AutomationRunMutability::ReadOnly => self.read_only,
        }
    }
}

pub(crate) fn set_server_api_url(url: String) {
    let _ = SERVER_API_URL.set(url);
}

fn patchbay_cli_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("PATCHBAY_CLI_PATH")
        .or_else(|| std::env::var_os("PATCHBAY_CLI"))
        .map(PathBuf::from)
    {
        return ensure_patchbay_cli_path(path);
    }

    let dev_script_search = find_dev_patchbay_cli();
    if let Some(dev_script) = dev_script_search.path {
        return ensure_patchbay_cli_path(dev_script);
    }

    let searched = dev_script_search
        .searched
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "Patchbay agent-facing CLI is not configured; set PATCHBAY_CLI_PATH or create dev-bin/patchbay (searched: {searched})"
    )
}

#[derive(Debug)]
struct DevPatchbayCliSearch {
    path: Option<PathBuf>,
    searched: Vec<PathBuf>,
}

fn find_dev_patchbay_cli() -> DevPatchbayCliSearch {
    let mut roots = Vec::new();
    if let Ok(current_dir) = std::env::current_dir() {
        roots.push(current_dir);
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    if let Ok(current_exe) = std::env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        roots.push(parent.to_path_buf());
    }
    find_dev_patchbay_cli_from_roots(roots)
}

fn find_dev_patchbay_cli_from_roots(
    roots: impl IntoIterator<Item = PathBuf>,
) -> DevPatchbayCliSearch {
    let mut seen = HashSet::new();
    let mut searched = Vec::new();
    for root in roots {
        for ancestor in root.ancestors() {
            let candidate = ancestor.join("dev-bin/patchbay");
            if !seen.insert(candidate.clone()) {
                continue;
            }
            if candidate.is_file() {
                return DevPatchbayCliSearch {
                    path: Some(candidate),
                    searched,
                };
            }
            searched.push(candidate);
        }
    }
    DevPatchbayCliSearch {
        path: None,
        searched,
    }
}

fn ensure_patchbay_cli_path(path: PathBuf) -> Result<PathBuf> {
    if !path.is_file() {
        bail!(
            "Patchbay agent-facing CLI path '{}' does not exist or is not a file",
            path.display()
        );
    }
    Ok(path)
}

fn ensure_tool_supports_mutability(
    tool: AgentToolName,
    mutability: AutomationRunMutability,
) -> Result<()> {
    match (tool, mutability) {
        (AgentToolName::Codex, AutomationRunMutability::Mutating)
        | (AgentToolName::Codex, AutomationRunMutability::ReadOnly) => Ok(()),
    }
}

pub async fn start_automation_with_sessions_until(
    store: &Store,
    project_name: &str,
    start: StartAutomation,
    sessions: Option<ProcessSessionRegistry>,
    cancellation: Option<watch::Receiver<bool>>,
) -> Result<AgentRunView> {
    let started = begin_automation_run(store, project_name, start).await?;
    complete_started_automation_run(store, started, sessions, cancellation).await
}

pub async fn start_one_automation_run_in_background(
    store: Store,
    project_name: String,
    start: StartAutomation,
    sessions: Option<ProcessSessionRegistry>,
) -> Result<AgentRunView> {
    let started = begin_automation_run(&store, &project_name, start).await?;
    let initial_run = model_to_view(started.run.clone())?;
    let project_for_task = started.project_name.clone();
    tokio::spawn(async move {
        match complete_started_automation_run(&store, started, sessions, None).await {
            Ok(run) if run.status == AgentRunStatus::Failed => {
                tracing::error!(
                    run_id = run.id,
                    project = %project_for_task,
                    summary = %run.result_summary,
                    "automation run failed"
                );
            }
            Ok(run) if run.status == AgentRunStatus::Cancelled => {
                tracing::warn!(
                    run_id = run.id,
                    project = %project_for_task,
                    summary = %run.result_summary,
                    "automation run cancelled"
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(
                    project = %project_for_task,
                    error = %format_args!("{err:#}"),
                    "automation run failed"
                );
            }
        }
    });
    Ok(initial_run)
}

async fn begin_automation_run(
    store: &Store,
    project_name: &str,
    start: StartAutomation,
) -> Result<StartedAutomationRun> {
    let project = projects::get_project(store, project_name).await?;
    let settings = projects::get_settings(store, project_name).await?;
    let mutability = start
        .mutability
        .unwrap_or(AutomationRunMutability::Mutating);
    let tool = start.tool.unwrap_or(settings.default_agent_tool);
    ensure_tool_supports_mutability(tool, mutability)?;
    enforce_concurrency(store, project_name, &settings, mutability).await?;
    let run = create_run(store, project.id, tool, mutability, start.trigger.as_ref()).await?;

    Ok(StartedAutomationRun {
        project_name: project_name.to_owned(),
        project,
        settings,
        start,
        tool,
        run,
    })
}

async fn complete_started_automation_run(
    store: &Store,
    started: StartedAutomationRun,
    sessions: Option<ProcessSessionRegistry>,
    cancellation: Option<watch::Receiver<bool>>,
) -> Result<AgentRunView> {
    let StartedAutomationRun {
        project_name,
        project,
        settings,
        start,
        tool,
        mut run,
    } = started;
    let agent_id = agent_ids::patchbay_run_agent_id(run.id);
    let run_mutability = AutomationRunMutability::from_str(&run.mutability)?;

    if cancellation_requested(&cancellation) {
        return cancel_run(
            store,
            run,
            "Automation run cancelled before startup".to_owned(),
        )
        .await;
    }

    let codex_binary = match agent_tools::resolve_tool_path(store, tool).await {
        Ok(codex_binary) => codex_binary,
        Err(err) => {
            return fail_run(
                store,
                run,
                format!("Failed to resolve automation tool: {err:#}"),
            )
            .await;
        }
    };
    let patchbay_binary = match patchbay_cli_path() {
        Ok(path) => path,
        Err(err) => {
            return fail_run(
                store,
                run,
                format!("Failed to resolve Patchbay CLI for automation: {err:#}"),
            )
            .await;
        }
    };
    if let Err(err) = codex_app_server::ensure_app_server_usable(&codex_binary).await {
        return fail_run(
            store,
            run,
            format!("Codex automation preconditions failed: {err:#}"),
        )
        .await;
    }
    if cancellation_requested(&cancellation) {
        return cancel_run(
            store,
            run,
            "Automation run cancelled before claiming work".to_owned(),
        )
        .await;
    }
    let project_path = match project
        .path
        .as_ref()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
    {
        Some(path) => path,
        None => {
            return fail_run(
                store,
                run,
                format!("Failed to start automation: project '{project_name}' has no path"),
            )
            .await;
        }
    };

    let claimed_item = {
        let claimed = if let Some(work_item_id) = start.work_item_id {
            match item_claims::claim_specific_item(store, &project_name, work_item_id, &agent_id)
                .await
            {
                Ok(claimed) => claimed,
                Err(err) => {
                    return fail_run(
                        store,
                        run,
                        format!("Failed to claim work item {work_item_id}: {err:#}"),
                    )
                    .await;
                }
            }
        } else if let Some(condition) = start.work_item_selector.as_ref() {
            match item_claims::claim_item_matching_condition(
                store,
                &project_name,
                &agent_id,
                condition,
            )
            .await
            {
                Ok(claimed) => claimed,
                Err(err) => {
                    return fail_run(
                        store,
                        run,
                        format!("Failed to claim work item matching automation selector: {err:#}"),
                    )
                    .await;
                }
            }
        } else {
            match item_claims::claim_item(store, &project_name, &agent_id, DEFAULT_STATE_LABEL)
                .await
            {
                Ok(claimed) => claimed,
                Err(err) => {
                    return fail_run(
                        store,
                        run,
                        format!("Failed to claim open work item: {err:#}"),
                    )
                    .await;
                }
            }
        };
        if claimed.is_none() {
            run = finish_run(
                store,
                run,
                AgentRunStatus::Completed,
                None,
                "No matching work item was available".to_owned(),
            )
            .await?;
            return model_to_view(run);
        }
        claimed
    };

    if let Some(item) = &claimed_item {
        let run_before_item_update = run.clone();
        run = match update_run_work_item_id(store, run, item.id).await {
            Ok(run) => run,
            Err(err) => {
                return fail_run_after_claim(
                    store,
                    &project_name,
                    run_before_item_update,
                    claimed_item.as_ref(),
                    &agent_id,
                    format!("Failed to attach claimed item to automation run: {err:#}"),
                )
                .await;
            }
        };
    }

    if cancellation_requested(&cancellation) {
        return cancel_run_after_claim(
            store,
            &project_name,
            run,
            claimed_item.as_ref(),
            &agent_id,
            "Automation run cancelled before launch".to_owned(),
        )
        .await;
    }

    let workspace = match prepare_workspace_for_run(
        run.id,
        &project_name,
        &project_path,
        settings.workspace_mode,
        run_mutability,
    ) {
        Ok(workspace) => workspace,
        Err(err) => {
            let result_summary = format!("Failed to prepare workspace: {err}");
            release_claim_if_needed(
                store,
                ClaimReleaseContext {
                    project_name: &project_name,
                    run_id: run.id,
                    claimed_item: claimed_item.as_ref(),
                    agent_id: &agent_id,
                    reason: ClaimReleaseReason::Failed,
                    detail: Some(&result_summary),
                    automation_disposition: item_claims::ReleaseAutomationDisposition::Claimable,
                },
            )
            .await?;
            run = finish_run(store, run, AgentRunStatus::Failed, None, result_summary).await?;
            return model_to_view(run);
        }
    };

    let log_dir = automation_log_dir();
    if let Err(err) = fs::create_dir_all(&log_dir)
        .context_with(|| format!("failed to create automation log dir {}", log_dir.display()))
    {
        return fail_run_after_claim(
            store,
            &project_name,
            run,
            claimed_item.as_ref(),
            &agent_id,
            format!("Failed to create automation log directory: {err:#}"),
        )
        .await;
    }
    let prompt_path = log_dir.join(format!("run-{}.prompt.md", run.id));
    let log_path = log_dir.join(format!("run-{}.output.json", run.id));
    let agent_model = effective_agent_model(&settings, claimed_item.as_ref());
    let agent_reasoning_effort = effective_agent_reasoning_effort(&settings, claimed_item.as_ref());
    let codex_home = match codex_app_server::ensure_project_codex_home(&settings) {
        Ok(codex_home) => codex_home,
        Err(err) => {
            return fail_run_after_claim(
                store,
                &project_name,
                run,
                claimed_item.as_ref(),
                &agent_id,
                format!("Failed to prepare project Codex home: {err:#}"),
            )
            .await;
        }
    };
    let real_git_path = match resolve_real_git_path() {
        Ok(real_git_path) => real_git_path,
        Err(err) => {
            return fail_run_after_claim(
                store,
                &project_name,
                run,
                claimed_item.as_ref(),
                &agent_id,
                format!("Failed to resolve git for automation: {err:#}"),
            )
            .await;
        }
    };
    let git_runtime = match prepare_git_runtime(
        run.id,
        &log_dir,
        &patchbay_binary,
        &settings,
        run_mutability,
    ) {
        Ok(git_runtime) => git_runtime,
        Err(err) => {
            return fail_run_after_claim(
                store,
                &project_name,
                run,
                claimed_item.as_ref(),
                &agent_id,
                format!("Failed to prepare git policy wrapper: {err:#}"),
            )
            .await;
        }
    };
    let memory_event_id = match projects::latest_memory_event_id(store, project.id).await {
        Ok(memory_event_id) => memory_event_id,
        Err(err) => {
            return fail_run_after_claim(
                store,
                &project_name,
                run,
                claimed_item.as_ref(),
                &agent_id,
                format!("Failed to resolve project memory event: {err:#}"),
            )
            .await;
        }
    };
    let personality_description = match personalities::personality_description_for_prompt(
        store,
        project.id,
        start.personality_id,
    )
    .await
    {
        Ok(personality_description) => personality_description,
        Err(err) => {
            return fail_run_after_claim(
                store,
                &project_name,
                run,
                claimed_item.as_ref(),
                &agent_id,
                format!("Failed to resolve automation personality: {err:#}"),
            )
            .await;
        }
    };
    let prompt_git_policy = git_runtime_policy_for_run(&settings, run_mutability);
    let prompt = build_prompt(PromptContext {
        project_name: &project_name,
        system_prompt: &project.system_prompt,
        memory: &project.memory,
        memory_event_id,
        item: claimed_item.as_ref(),
        agent_id: &agent_id,
        personality_description: personality_description.as_deref(),
        extra_prompt: start.extra_prompt.as_deref(),
        mutability: run_mutability,
        workspace_mode: settings.workspace_mode,
        auto_commit: settings.auto_commit,
        commit_standard: &settings.commit_standard,
        revert_strategy: settings.revert_strategy,
        create_pr: settings.create_pr,
        git_command_policy: prompt_git_policy.policy,
        git_policy_workspace_mode: prompt_git_policy.workspace_mode,
    });
    if let Err(err) = fs::write(&prompt_path, prompt)
        .context_with(|| format!("failed to write prompt {}", prompt_path.display()))
    {
        return fail_run_after_claim(
            store,
            &project_name,
            run,
            claimed_item.as_ref(),
            &agent_id,
            format!("Failed to write automation prompt: {err:#}"),
        )
        .await;
    }

    let command = format!(
        "{} app-server turn {}",
        codex_binary.display(),
        prompt_path.display()
    );
    let run_before_launch_update = run.clone();
    run = match update_run_launch_details(
        store,
        run,
        LaunchDetails {
            work_item_id: claimed_item.as_ref().map(|item| item.id),
            command,
            workspace,
            prompt_path: Some(prompt_path.to_string_lossy().into_owned()),
            log_path: Some(log_path.to_string_lossy().into_owned()),
            memory_event_id,
            agent_model: agent_model.clone(),
            agent_reasoning_effort,
            commit_required: commit_required_for_run(&settings, run_mutability),
            pr_requested: pr_requested_for_run(&settings, run_mutability),
        },
    )
    .await
    {
        Ok(run) => run,
        Err(err) => {
            return fail_run_after_claim(
                store,
                &project_name,
                run_before_launch_update,
                claimed_item.as_ref(),
                &agent_id,
                format!("Failed to update automation launch details: {err:#}"),
            )
            .await;
        }
    };

    let commit_baseline = capture_commit_baseline(
        Path::new(&run.working_dir),
        commit_required_for_run(&settings, run_mutability),
    );
    let output = run_agent_process(
        AgentProcessStart {
            run_id: run.id,
            project_name: project_name.clone(),
            tool_name: tool,
            codex_binary,
            codex_home,
            patchbay_binary,
            prompt_path,
            working_dir: PathBuf::from(&run.working_dir),
            git_shim_dir: git_runtime.shim_dir,
            git_policy_path: git_runtime.policy_path,
            real_git_path,
            agent_id: agent_id.clone(),
            claimed_item_id: claimed_item.as_ref().map(|item| item.id),
            agent_model,
            agent_reasoning_effort,
            agent_sandbox_mode: settings.agent_sandbox_mode,
            agent_extra_writable_roots: settings.agent_extra_writable_roots.clone(),
            mutability: run_mutability,
        },
        sessions,
        cancellation,
    )
    .await;
    match output {
        Ok(output) => {
            run = update_run_process_id(store, run, output.process_id).await?;
            if let Some(token_usage) = output.token_usage {
                run = update_run_token_usage(store, run, token_usage).await?;
            }
            write_run_output_log(&log_path, &output.output).context_with(|| {
                format!("failed to write automation log {}", log_path.display())
            })?;
            let exit_code = Some(0);
            let mut success = true;
            let mut result_summary = if output.final_response.trim().is_empty() {
                "Codex app-server turn completed successfully".to_owned()
            } else {
                "Codex app-server turn completed successfully with a final response".to_owned()
            };
            let commit_evaluation = evaluate_commit_outcome_for_run(
                Path::new(&run.working_dir),
                &commit_baseline,
                run_mutability,
            );
            run = update_run_commit_outcome(store, run, &commit_evaluation).await?;
            if commit_evaluation.validation_failed {
                success = false;
                result_summary = format!(
                    "Codex app-server turn completed, but required git commit is missing: {}",
                    commit_evaluation
                        .detail
                        .as_deref()
                        .unwrap_or("no new commit was created")
                );
            }
            if success && pr_requested_for_run(&settings, run_mutability) {
                match create_pull_request(Path::new(&run.working_dir)).await {
                    Ok(pr_url) => {
                        result_summary = format!(
                            "Codex app-server turn completed successfully; PR created: {pr_url}"
                        );
                        run = update_run_pr_url(store, run, Some(pr_url)).await?;
                    }
                    Err(err) => {
                        success = false;
                        result_summary = format!(
                            "Codex app-server turn completed, but PR creation failed: {err}"
                        );
                    }
                }
            }
            release_claim_if_needed(
                store,
                ClaimReleaseContext {
                    project_name: &project_name,
                    run_id: run.id,
                    claimed_item: claimed_item.as_ref(),
                    agent_id: &agent_id,
                    reason: if success {
                        ClaimReleaseReason::Completed
                    } else {
                        ClaimReleaseReason::Failed
                    },
                    detail: Some(&result_summary),
                    automation_disposition: if success {
                        item_claims::ReleaseAutomationDisposition::Claimable
                    } else {
                        item_claims::ReleaseAutomationDisposition::Blocked
                    },
                },
            )
            .await?;
            run = finish_run(
                store,
                run,
                if success {
                    AgentRunStatus::Completed
                } else {
                    AgentRunStatus::Failed
                },
                exit_code,
                result_summary,
            )
            .await?;
            if success && settings.worktree_cleanup_policy == WorktreeCleanupPolicy::AfterSuccess {
                run = cleanup_worktree_for_run(store, run, &project_path).await?;
            }
            model_to_view(run)
        }
        Err(err) => {
            let cancelled = is_automation_cancelled(&err);
            let message = if cancelled {
                "Automation run cancelled".to_owned()
            } else {
                format!("Failed to launch or run Codex app-server turn: {err}")
            };
            let output = vec![new_output_piece(
                1,
                AgentRunOutputKind::Error,
                None,
                if cancelled { "cancelled" } else { "error" },
                message.clone(),
                serde_json::json!({ "cancelled": cancelled }),
            )];
            write_run_output_log(&log_path, &output).context_with(|| {
                format!("failed to write automation log {}", log_path.display())
            })?;
            let commit_evaluation = evaluate_commit_outcome_for_run(
                Path::new(&run.working_dir),
                &commit_baseline,
                run_mutability,
            );
            run = update_run_commit_outcome(store, run, &commit_evaluation).await?;
            release_claim_if_needed(
                store,
                ClaimReleaseContext {
                    project_name: &project_name,
                    run_id: run.id,
                    claimed_item: claimed_item.as_ref(),
                    agent_id: &agent_id,
                    reason: if cancelled {
                        ClaimReleaseReason::Cancelled
                    } else {
                        ClaimReleaseReason::Failed
                    },
                    detail: Some(&message),
                    automation_disposition: if cancelled {
                        item_claims::ReleaseAutomationDisposition::Claimable
                    } else {
                        item_claims::ReleaseAutomationDisposition::Blocked
                    },
                },
            )
            .await?;
            run = finish_run(
                store,
                run,
                if cancelled {
                    AgentRunStatus::Cancelled
                } else {
                    AgentRunStatus::Failed
                },
                None,
                message,
            )
            .await?;
            model_to_view(run)
        }
    }
}

async fn fail_run(
    store: &Store,
    run: AgentRunModel,
    result_summary: String,
) -> Result<AgentRunView> {
    let run = finish_run(store, run, AgentRunStatus::Failed, None, result_summary).await?;
    model_to_view(run)
}

async fn cancel_run(
    store: &Store,
    run: AgentRunModel,
    result_summary: String,
) -> Result<AgentRunView> {
    let run = finish_run(store, run, AgentRunStatus::Cancelled, None, result_summary).await?;
    model_to_view(run)
}

async fn fail_run_after_claim(
    store: &Store,
    project_name: &str,
    run: AgentRunModel,
    claimed_item: Option<&WorkItemView>,
    agent_id: &str,
    result_summary: String,
) -> Result<AgentRunView> {
    release_claim_if_needed(
        store,
        ClaimReleaseContext {
            project_name,
            run_id: run.id,
            claimed_item,
            agent_id,
            reason: ClaimReleaseReason::Failed,
            detail: Some(&result_summary),
            automation_disposition: item_claims::ReleaseAutomationDisposition::Claimable,
        },
    )
    .await?;
    fail_run(store, run, result_summary).await
}

async fn cancel_run_after_claim(
    store: &Store,
    project_name: &str,
    run: AgentRunModel,
    claimed_item: Option<&WorkItemView>,
    agent_id: &str,
    result_summary: String,
) -> Result<AgentRunView> {
    release_claim_if_needed(
        store,
        ClaimReleaseContext {
            project_name,
            run_id: run.id,
            claimed_item,
            agent_id,
            reason: ClaimReleaseReason::Cancelled,
            detail: Some(&result_summary),
            automation_disposition: item_claims::ReleaseAutomationDisposition::Claimable,
        },
    )
    .await?;
    cancel_run(store, run, result_summary).await
}

pub async fn stop_automation(store: &Store, project_name: &str) -> Result<Vec<AgentRunView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let running = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .filter(agent_run::Column::Status.eq(AgentRunStatus::Running.as_storage()))
        .all(store.db().as_ref())
        .await
        .context("failed to load running agent runs")?;

    let mut cancelled = Vec::new();
    for run in running {
        let run_id = run.id;
        let agent_id = agent_ids::patchbay_run_agent_id(run.id);
        let claimed_item = match run.work_item_id {
            Some(item_id) => Some(items::get_item(store, project_name, item_id).await?),
            None => None,
        };
        let result_summary = "Marked cancelled by automation stop".to_owned();
        release_claim_if_needed(
            store,
            ClaimReleaseContext {
                project_name,
                run_id,
                claimed_item: claimed_item.as_ref(),
                agent_id: &agent_id,
                reason: ClaimReleaseReason::Cancelled,
                detail: Some(&result_summary),
                automation_disposition: item_claims::ReleaseAutomationDisposition::Claimable,
            },
        )
        .await?;
        let updated =
            finish_run(store, run, AgentRunStatus::Cancelled, None, result_summary).await?;
        cancelled.push(model_to_view(updated)?);
    }
    Ok(cancelled)
}

pub async fn automation_status(store: &Store, project_name: &str) -> Result<AutomationStatusView> {
    let settings = projects::get_settings(store, project_name).await?;
    let running_counts = running_run_counts(store, project_name).await?;
    let recent_runs = list_runs(store, project_name, Some(10)).await?;
    let tools = agent_tools::list_tools(store).await?;

    Ok(AutomationStatusView {
        project: project_name.to_owned(),
        allowed_mutating_runs: projects::allowed_code_edit_agents(&settings),
        settings,
        running_runs: running_counts.total(),
        running_mutating_runs: running_counts.mutating,
        running_read_only_runs: running_counts.read_only,
        recent_runs,
        tools,
    })
}

pub async fn active_project_names(store: &Store) -> Result<Vec<String>> {
    let projects = projects::list_projects(store).await?;
    let mut active = Vec::new();
    for project in projects {
        if running_run_counts(store, &project.name).await?.total() > 0 {
            active.push(project.name);
        }
    }
    Ok(active)
}

pub async fn list_runs(
    store: &Store,
    project_name: &str,
    limit: Option<u64>,
) -> Result<Vec<AgentRunView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let mut query = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .order_by_desc(agent_run::Column::CreatedAt)
        .order_by_desc(agent_run::Column::Id);
    if let Some(limit) = limit {
        query = query.limit(limit);
    }

    let runs = query
        .all(store.db().as_ref())
        .await
        .context("failed to list agent runs")?;
    let mut views = Vec::with_capacity(runs.len());
    for run in runs {
        views.push(model_to_view_with_log_usage(run).await?);
    }
    Ok(views)
}

pub async fn list_runs_for_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    limit: Option<u64>,
) -> Result<Vec<AgentRunView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let mut query = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .filter(agent_run::Column::WorkItemId.eq(item_id))
        .order_by_desc(agent_run::Column::CreatedAt)
        .order_by_desc(agent_run::Column::Id);
    if let Some(limit) = limit {
        query = query.limit(limit);
    }

    let runs = query
        .all(store.db().as_ref())
        .await
        .context("failed to list item agent runs")?;
    let mut views = Vec::with_capacity(runs.len());
    for run in runs {
        views.push(model_to_view_with_log_usage(run).await?);
    }
    Ok(views)
}

pub async fn list_runs_for_trigger(
    store: &Store,
    project_name: &str,
    trigger_id: i64,
    limit: Option<u64>,
) -> Result<Vec<AgentRunView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let mut query = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .filter(agent_run::Column::TriggerId.eq(trigger_id))
        .order_by_desc(agent_run::Column::CreatedAt)
        .order_by_desc(agent_run::Column::Id);
    if let Some(limit) = limit {
        query = query.limit(limit);
    }

    let runs = query
        .all(store.db().as_ref())
        .await
        .context("failed to list trigger agent runs")?;
    let mut views = Vec::with_capacity(runs.len());
    for run in runs {
        views.push(model_to_view_with_log_usage(run).await?);
    }
    Ok(views)
}

pub async fn get_run(store: &Store, project_name: &str, run_id: i64) -> Result<AgentRunView> {
    let project_id = projects::project_id(store, project_name).await?;
    let run = AgentRun::find_by_id(run_id)
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context("failed to load agent run")?
        .ok_or_else(|| report!("agent run {run_id} does not exist in this project"))?;
    model_to_view_with_log_usage(run).await
}

pub async fn read_run_log(store: &Store, project_name: &str, run_id: i64) -> Result<RunLogView> {
    let run = get_run(store, project_name, run_id).await?;
    let prompt = read_optional_text(run.prompt_path.as_deref()).await?;
    let output = read_run_output(run.log_path.as_deref()).await?;
    let memory_event = match run.memory_event_id {
        Some(event_id) => {
            let created_at = projects::memory_event_exists(store, run.project_id, event_id).await?;
            Some(ProjectMemoryEventRefView {
                event_id,
                available: created_at.is_some(),
                created_at,
            })
        }
        None => None,
    };
    Ok(RunLogView {
        run,
        active: false,
        memory_event,
        prompt,
        output,
    })
}

pub async fn read_run_log_with_active_session(
    store: &Store,
    sessions: &ProcessSessionRegistry,
    project_name: &str,
    run_id: i64,
) -> Result<RunLogView> {
    let mut run_log = read_run_log(store, project_name, run_id).await?;
    if let Some(session) = sessions.get_for_project(project_name, run_id).await {
        run_log.active = true;
        if !session.output.is_empty() {
            run_log.output = session.output;
        }
    }
    Ok(run_log)
}

pub async fn cleanup_worktrees(
    store: &Store,
    project_name: &str,
    run_id: Option<i64>,
) -> Result<Vec<AgentRunView>> {
    let project = projects::get_project(store, project_name).await?;
    let project_id = project.id;
    let project_path = project
        .path
        .as_ref()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| report!("project '{project_name}' has no path"))?;
    let mut query = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .order_by_desc(agent_run::Column::CreatedAt)
        .order_by_desc(agent_run::Column::Id);
    if let Some(run_id) = run_id {
        query = query.filter(agent_run::Column::Id.eq(run_id));
    }

    let runs = query
        .all(store.db().as_ref())
        .await
        .context("failed to load agent runs for cleanup")?;
    let mut cleaned = Vec::new();
    for run in runs {
        cleaned.push(
            model_to_view_with_log_usage(
                cleanup_worktree_for_run(store, run, &project_path).await?,
            )
            .await?,
        );
    }
    Ok(cleaned)
}

pub async fn recover_stale_claims_for_project(
    store: &Store,
    project_name: &str,
    stale_after_minutes: Option<i64>,
) -> Result<Vec<RecoveredClaimView>> {
    let minutes = match stale_after_minutes {
        Some(minutes) => minutes,
        None => {
            projects::get_settings(store, project_name)
                .await?
                .stale_claim_minutes
        }
    };
    item_claims::recover_stale_claims(store, project_name, minutes).await
}

pub async fn recover_configured_stale_claims(store: &Store) -> Result<Vec<RecoveredClaimView>> {
    let projects = projects::list_projects(store).await?;
    let mut recovered = Vec::new();
    for project in projects {
        let settings = projects::get_settings(store, &project.name).await?;
        if settings.stale_claim_minutes > 0 {
            recovered.extend(
                item_claims::recover_stale_claims(
                    store,
                    &project.name,
                    settings.stale_claim_minutes,
                )
                .await?,
            );
        }
    }
    Ok(recovered)
}

async fn enforce_concurrency(
    store: &Store,
    project_name: &str,
    settings: &ProjectSettingsView,
    mutability: AutomationRunMutability,
) -> Result<()> {
    if mutability == AutomationRunMutability::Mutating
        && settings.create_pr
        && settings.workspace_mode == WorkspaceMode::CurrentBranch
    {
        bail!("pull requests can only be created for git_worktree or git_branch strategies");
    }
    let allowed = allowed_runs_for_mutability(settings, mutability);
    let running = running_run_counts(store, project_name)
        .await?
        .for_mutability(mutability);
    if running >= allowed {
        match mutability {
            AutomationRunMutability::Mutating => {
                bail!(
                    "project already has {running} running mutating agent run(s); limit is {allowed}"
                );
            }
            AutomationRunMutability::ReadOnly => {
                bail!(
                    "project already has {running} running read-only agent run(s); limit is {allowed}"
                );
            }
        }
    }
    Ok(())
}

pub async fn can_start_automation_run(
    store: &Store,
    project_name: &str,
    mutability: AutomationRunMutability,
) -> Result<bool> {
    let settings = projects::get_settings(store, project_name).await?;
    let allowed = allowed_runs_for_mutability(&settings, mutability);
    let running = running_run_counts(store, project_name)
        .await?
        .for_mutability(mutability);
    Ok(running < allowed)
}

fn allowed_runs_for_mutability(
    settings: &ProjectSettingsView,
    mutability: AutomationRunMutability,
) -> i64 {
    match mutability {
        AutomationRunMutability::Mutating => projects::allowed_code_edit_agents(settings),
        AutomationRunMutability::ReadOnly => settings.max_read_only_agents,
    }
}

async fn running_run_counts(store: &Store, project_name: &str) -> Result<RunningRunCounts> {
    let project_id = projects::project_id(store, project_name).await?;
    let runs = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .filter(agent_run::Column::Status.eq(AgentRunStatus::Running.as_storage()))
        .all(store.db().as_ref())
        .await
        .context("failed to load running agent runs")?;
    let mut counts = RunningRunCounts::default();
    for run in runs {
        match AutomationRunMutability::from_str(&run.mutability)? {
            AutomationRunMutability::Mutating => counts.mutating += 1,
            AutomationRunMutability::ReadOnly => counts.read_only += 1,
        }
    }
    Ok(counts)
}

async fn create_run(
    store: &Store,
    project_id: i64,
    tool: AgentToolName,
    mutability: AutomationRunMutability,
    trigger: Option<&AutomationTriggerOrigin>,
) -> Result<AgentRunModel> {
    let now = utc_now();
    let run = AgentRunActiveModel {
        project_id: Set(project_id),
        work_item_id: Set(None),
        memory_event_id: Set(None),
        trigger_id: Set(trigger.map(|trigger| trigger.trigger_id)),
        trigger_name: Set(trigger.map(|trigger| trigger.trigger_name.clone())),
        tool_name: Set(tool.as_storage().to_owned()),
        mutability: Set(mutability.as_storage().to_owned()),
        status: Set(AgentRunStatus::Running.as_storage().to_owned()),
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
        commit_outcome: Set(AgentCommitOutcome::NotEvaluated.as_storage().to_owned()),
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
    .context("failed to create agent run")?;
    publish_run_model_event(store, &run).await;
    Ok(run)
}

async fn update_run_launch_details(
    store: &Store,
    run: AgentRunModel,
    details: LaunchDetails,
) -> Result<AgentRunModel> {
    let mut active: AgentRunActiveModel = run.into();
    active.work_item_id = Set(details.work_item_id);
    active.memory_event_id = Set(details.memory_event_id);
    active.command = Set(details.command);
    active.working_dir = Set(details.workspace.working_dir.to_string_lossy().into_owned());
    let has_worktree = details.workspace.worktree_path.is_some();
    active.worktree_path = Set(details
        .workspace
        .worktree_path
        .map(|path| path.to_string_lossy().into_owned()));
    active.branch_name = Set(details.workspace.branch_name);
    active.prompt_path = Set(details.prompt_path);
    active.log_path = Set(details.log_path);
    active.agent_model = Set(details.agent_model);
    active.agent_reasoning_effort = Set(details
        .agent_reasoning_effort
        .map(|effort| effort.as_storage().to_owned()));
    active.commit_required = Set(details.commit_required);
    active.pr_requested = Set(details.pr_requested);
    active.cleanup_status = Set(if has_worktree {
        "pending".to_owned()
    } else {
        "not_applicable".to_owned()
    });
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update agent run launch details")?;
    publish_run_model_event(store, &updated).await;
    Ok(updated)
}

async fn update_run_work_item_id(
    store: &Store,
    run: AgentRunModel,
    work_item_id: i64,
) -> Result<AgentRunModel> {
    let mut active: AgentRunActiveModel = run.into();
    active.work_item_id = Set(Some(work_item_id));
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update agent run work item")?;
    publish_run_model_event(store, &updated).await;
    Ok(updated)
}

async fn finish_run(
    store: &Store,
    run: AgentRunModel,
    status: AgentRunStatus,
    exit_code: Option<i64>,
    result_summary: String,
) -> Result<AgentRunModel> {
    let now = utc_now();
    let mut active: AgentRunActiveModel = run.into();
    active.status = Set(status.as_storage().to_owned());
    active.exit_code = Set(exit_code);
    active.result_summary = Set(result_summary);
    active.finished_at = Set(Some(now.clone()));
    active.updated_at = Set(now);
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to finish agent run")?;
    publish_run_model_event(store, &updated).await;
    Ok(updated)
}

async fn update_run_process_id(
    store: &Store,
    run: AgentRunModel,
    process_id: Option<i64>,
) -> Result<AgentRunModel> {
    let mut active: AgentRunActiveModel = run.into();
    active.process_id = Set(process_id);
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update agent run process id")?;
    publish_run_model_event(store, &updated).await;
    Ok(updated)
}

async fn update_run_token_usage(
    store: &Store,
    run: AgentRunModel,
    usage: AgentRunTokenUsageView,
) -> Result<AgentRunModel> {
    let mut active: AgentRunActiveModel = run.into();
    active.input_tokens = Set(Some(usage.input_tokens));
    active.cached_input_tokens = Set(Some(usage.cached_input_tokens));
    active.output_tokens = Set(Some(usage.output_tokens));
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update agent run token usage")?;
    publish_run_model_event(store, &updated).await;
    Ok(updated)
}

async fn update_run_commit_outcome(
    store: &Store,
    run: AgentRunModel,
    evaluation: &CommitOutcomeEvaluation,
) -> Result<AgentRunModel> {
    let mut active: AgentRunActiveModel = run.into();
    active.commit_outcome = Set(evaluation.outcome.as_storage().to_owned());
    active.commit_shas = Set(serde_json::to_string(&evaluation.shas)
        .context("failed to encode automation commit SHAs")?);
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update agent run commit outcome")?;
    publish_run_model_event(store, &updated).await;
    Ok(updated)
}

async fn publish_run_model_event(store: &Store, run: &AgentRunModel) {
    match projects::project_name_by_id(store, run.project_id).await {
        Ok(project_name) => {
            events::publish_agent_run_changed(&project_name, run.id, run.work_item_id);
        }
        Err(err) => {
            tracing::warn!(
                error = %format_args!("{err:#}"),
                "failed to resolve project for agent run UI event"
            );
        }
    }
}

fn prepare_workspace_for_run(
    run_id: i64,
    project_name: &str,
    project_path: &Path,
    workspace_mode: WorkspaceMode,
    mutability: AutomationRunMutability,
) -> Result<WorkspacePlan> {
    if mutability == AutomationRunMutability::ReadOnly {
        return prepare_read_only_workspace(project_path);
    }
    prepare_workspace(run_id, project_name, project_path, workspace_mode)
}

fn prepare_read_only_workspace(project_path: &Path) -> Result<WorkspacePlan> {
    if !project_path.is_dir() {
        bail!("path '{}' is not a directory", project_path.display());
    }
    Ok(WorkspacePlan {
        working_dir: project_path.to_path_buf(),
        worktree_path: None,
        branch_name: None,
    })
}

fn prepare_workspace(
    run_id: i64,
    project_name: &str,
    project_path: &Path,
    workspace_mode: WorkspaceMode,
) -> Result<WorkspacePlan> {
    if !project_path.is_dir() {
        bail!("path '{}' is not a directory", project_path.display());
    }

    match workspace_mode {
        WorkspaceMode::CurrentBranch => Ok(WorkspacePlan {
            working_dir: project_path.to_path_buf(),
            worktree_path: None,
            branch_name: None,
        }),
        WorkspaceMode::GitWorktree => {
            let slug = slugify(project_name);
            let root = project_path
                .parent()
                .unwrap_or(project_path)
                .join(".patchbay-worktrees");
            let worktree_path = root.join(format!("{slug}-{run_id}"));
            let branch_name = format!("patchbay/{slug}-{run_id}");
            fs::create_dir_all(&root)
                .context_with(|| format!("failed to create {}", root.display()))?;
            create_git_worktree(project_path, &branch_name, &worktree_path)?;
            Ok(WorkspacePlan {
                working_dir: worktree_path.clone(),
                worktree_path: Some(worktree_path),
                branch_name: Some(branch_name),
            })
        }
        WorkspaceMode::GitBranch => {
            let branch_name = format!("patchbay/{}-{}", slugify(project_name), run_id);
            create_and_checkout_git_branch(project_path, &branch_name)?;
            Ok(WorkspacePlan {
                working_dir: project_path.to_path_buf(),
                worktree_path: None,
                branch_name: Some(branch_name),
            })
        }
    }
}

async fn release_claim_if_needed(store: &Store, context: ClaimReleaseContext<'_>) -> Result<()> {
    let ClaimReleaseContext {
        project_name,
        run_id,
        claimed_item,
        agent_id,
        reason,
        detail,
        automation_disposition,
    } = context;
    let Some(claimed_item) = claimed_item else {
        return Ok(());
    };
    let current = items::get_item(store, project_name, claimed_item.id).await?;
    if current.claimed_by.as_deref() != Some(agent_id)
        || current.state.as_deref() == Some(FINISHED_STATE_LABEL)
    {
        return Ok(());
    }

    let base = match reason {
        ClaimReleaseReason::Completed => {
            "Automation turn completed without finishing the item; releasing claim."
        }
        ClaimReleaseReason::Failed => {
            "Automation turn failed before finishing the item; releasing claim."
        }
        ClaimReleaseReason::Cancelled => {
            "Automation turn was cancelled before finishing the item; releasing claim."
        }
    };
    let comment = claim_release_comment(base, run_id, detail);
    item_claims::release_item(
        store,
        project_name,
        claimed_item.id,
        agent_id,
        Some(comment),
        automation_disposition,
    )
    .await?;
    Ok(())
}

fn claim_release_comment(base: &str, run_id: i64, detail: Option<&str>) -> String {
    let mut comment = format!("{base} Run #{run_id}.");
    if let Some(detail) = detail
        .map(summarize_text)
        .filter(|detail| !detail.is_empty())
    {
        comment.push(' ');
        comment.push_str(&detail);
    }
    comment
}

fn effective_agent_model(
    settings: &ProjectSettingsView,
    item: Option<&WorkItemView>,
) -> Option<String> {
    item.and_then(|item| item.agent_model_override.clone())
        .or_else(|| settings.default_agent_model.clone())
}

fn effective_agent_reasoning_effort(
    settings: &ProjectSettingsView,
    item: Option<&WorkItemView>,
) -> Option<AgentReasoningEffort> {
    item.and_then(|item| item.agent_reasoning_effort_override)
        .or(settings.default_agent_reasoning_effort)
}

fn commit_required_for_policy(settings: &ProjectSettingsView) -> bool {
    match settings.workspace_mode {
        WorkspaceMode::CurrentBranch => settings.auto_commit,
        WorkspaceMode::GitBranch | WorkspaceMode::GitWorktree => true,
    }
}

fn commit_required_for_run(
    settings: &ProjectSettingsView,
    mutability: AutomationRunMutability,
) -> bool {
    match mutability {
        AutomationRunMutability::Mutating => commit_required_for_policy(settings),
        AutomationRunMutability::ReadOnly => false,
    }
}

fn pr_requested_for_run(
    settings: &ProjectSettingsView,
    mutability: AutomationRunMutability,
) -> bool {
    mutability == AutomationRunMutability::Mutating && settings.create_pr
}

fn to_codex_reasoning(effort: AgentReasoningEffort) -> CodexReasoningEffort {
    match effort {
        AgentReasoningEffort::None => CodexReasoningEffort::None,
        AgentReasoningEffort::Minimal => CodexReasoningEffort::Minimal,
        AgentReasoningEffort::Low => CodexReasoningEffort::Low,
        AgentReasoningEffort::Medium => CodexReasoningEffort::Medium,
        AgentReasoningEffort::High => CodexReasoningEffort::High,
        AgentReasoningEffort::XHigh => CodexReasoningEffort::XHigh,
    }
}

fn prepare_git_runtime(
    run_id: i64,
    log_dir: &Path,
    patchbay_binary: &Path,
    settings: &ProjectSettingsView,
    mutability: AutomationRunMutability,
) -> Result<GitRuntimeFiles> {
    let shim_dir = log_dir.join(format!("run-{run_id}-bin"));
    fs::create_dir_all(&shim_dir)
        .context_with(|| format!("failed to create git shim dir {}", shim_dir.display()))?;
    let policy_path = log_dir.join(format!("run-{run_id}.git-policy.json"));
    let runtime_policy = git_runtime_policy_for_run(settings, mutability);
    let policy_json = serde_json::to_string_pretty(&runtime_policy)
        .context("failed to encode git runtime policy")?;
    fs::write(&policy_path, policy_json)
        .context_with(|| format!("failed to write git policy {}", policy_path.display()))?;
    let shim_path = shim_dir.join("git");
    fs::write(
        &shim_path,
        format!(
            "#!/bin/sh\nexec {} git \"$@\"\n",
            shell_quote(&patchbay_binary.to_string_lossy())
        ),
    )
    .context_with(|| format!("failed to write git shim {}", shim_path.display()))?;
    mark_executable(&shim_path)?;
    Ok(GitRuntimeFiles {
        shim_dir,
        policy_path,
    })
}

fn git_runtime_policy_for_run(
    settings: &ProjectSettingsView,
    mutability: AutomationRunMutability,
) -> AgentGitRuntimePolicy {
    match mutability {
        AutomationRunMutability::Mutating => AgentGitRuntimePolicy {
            policy: settings.agent_git_command_policy.clone(),
            workspace_mode: settings.workspace_mode,
        },
        AutomationRunMutability::ReadOnly => AgentGitRuntimePolicy {
            policy: read_only_git_command_policy(),
            workspace_mode: WorkspaceMode::CurrentBranch,
        },
    }
}

fn read_only_git_command_policy() -> patchbay_types::AgentGitCommandPolicy {
    patchbay_types::AgentGitCommandPolicy {
        add: false,
        commit: false,
        push: false,
        reset: false,
        hard_reset: AgentGitHardResetPolicy::Never,
    }
}

fn resolve_real_git_path() -> Result<PathBuf> {
    let path = std::env::var_os("PATH").ok_or_else(|| report!("PATH is not set"))?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("git");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("git was not found on PATH")
}

#[cfg(unix)]
fn mark_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .context_with(|| format!("failed to stat {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    Ok(fs::set_permissions(path, permissions)
        .context_with(|| format!("failed to mark {} executable", path.display()))?)
}

#[cfg(not(unix))]
fn mark_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
    {
        return value.to_owned();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

fn agent_environment(
    patchbay_binary: &Path,
    git_runtime: &GitRuntimeFiles,
    real_git_path: &Path,
    project_name: &str,
    agent_id: &str,
    claimed_item_id: Option<i64>,
    api_url: Option<&str>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let path = std::env::var("PATH").unwrap_or_default();
    if let Some(bin_dir) = patchbay_binary.parent() {
        env.insert(
            "PATH".to_owned(),
            format!(
                "{}:{}:{path}",
                git_runtime.shim_dir.to_string_lossy(),
                bin_dir.to_string_lossy()
            ),
        );
    } else {
        env.insert(
            "PATH".to_owned(),
            format!("{}:{path}", git_runtime.shim_dir.to_string_lossy()),
        );
    }
    env.insert(
        "PATCHBAY_GIT_POLICY_PATH".to_owned(),
        git_runtime.policy_path.to_string_lossy().into_owned(),
    );
    env.insert(
        "PATCHBAY_REAL_GIT".to_owned(),
        real_git_path.to_string_lossy().into_owned(),
    );
    env.insert("PATCHBAY_PROJECT".to_owned(), project_name.to_owned());
    env.insert("PATCHBAY_AGENT_ID".to_owned(), agent_id.to_owned());
    if let Some(item_id) = claimed_item_id {
        env.insert("PATCHBAY_CLAIMED_ITEM_ID".to_owned(), item_id.to_string());
    }
    if let Some(api_url) = api_url {
        env.insert("PATCHBAY_API_URL".to_owned(), api_url.to_owned());
    }
    env
}

fn agent_sandbox_mode(mode: AgentSandboxMode) -> SandboxMode {
    match mode {
        AgentSandboxMode::WorkspaceWrite => SandboxMode::WorkspaceWrite,
        AgentSandboxMode::DangerFullAccess => SandboxMode::DangerFullAccess,
    }
}

fn agent_sandbox_mode_for_run(
    mutability: AutomationRunMutability,
    mode: AgentSandboxMode,
) -> SandboxMode {
    match mutability {
        AutomationRunMutability::Mutating => agent_sandbox_mode(mode),
        AutomationRunMutability::ReadOnly => SandboxMode::ReadOnly,
    }
}

fn agent_sandbox_policy(
    mode: AgentSandboxMode,
    agent_extra_writable_roots: &[String],
) -> serde_json::Value {
    match mode {
        AgentSandboxMode::WorkspaceWrite => serde_json::json!({
            "type": "workspaceWrite",
            "networkAccess": true,
            "writableRoots": agent_extra_writable_roots,
        }),
        AgentSandboxMode::DangerFullAccess => serde_json::json!({
            "type": "dangerFullAccess",
        }),
    }
}

fn agent_sandbox_policy_for_run(
    mutability: AutomationRunMutability,
    mode: AgentSandboxMode,
    agent_extra_writable_roots: &[String],
) -> serde_json::Value {
    match mutability {
        AutomationRunMutability::Mutating => agent_sandbox_policy(mode, agent_extra_writable_roots),
        AutomationRunMutability::ReadOnly => serde_json::json!({
            "type": "readOnly",
            "networkAccess": true,
        }),
    }
}

fn codex_memory_config_overrides() -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([
        (
            "features.memories".to_owned(),
            serde_json::Value::Bool(false),
        ),
        (
            "memories.use_memories".to_owned(),
            serde_json::Value::Bool(false),
        ),
        (
            "memories.generate_memories".to_owned(),
            serde_json::Value::Bool(false),
        ),
    ])
}

async fn run_agent_process(
    start: AgentProcessStart,
    sessions: Option<ProcessSessionRegistry>,
    external_cancellation: Option<watch::Receiver<bool>>,
) -> Result<AgentProcessOutput> {
    let command_label = format!(
        "{} app-server turn {}",
        start.codex_binary.display(),
        start.prompt_path.display()
    );
    let session_cancellation = if let Some(registry) = &sessions {
        Some(
            registry
                .begin(ProcessSessionStart {
                    run_id: start.run_id,
                    project_name: start.project_name.clone(),
                    tool_name: start.tool_name.to_string(),
                    command: command_label.clone(),
                    working_dir: start.working_dir.to_string_lossy().into_owned(),
                })
                .await,
        )
    } else {
        None
    };
    let run_id = start.run_id;

    let result = run_agent_process_inner(
        start,
        sessions.clone(),
        session_cancellation,
        external_cancellation,
    )
    .await;

    if let Some(registry) = &sessions {
        registry.finish(run_id).await;
    }
    result
}

async fn run_agent_process_inner(
    start: AgentProcessStart,
    sessions: Option<ProcessSessionRegistry>,
    session_cancellation: Option<watch::Receiver<bool>>,
    external_cancellation: Option<watch::Receiver<bool>>,
) -> Result<AgentProcessOutput> {
    run_agent_process_turn_with_cancellation(
        run_codex_app_server_turn(start, sessions),
        session_cancellation,
        external_cancellation,
    )
    .await
}

async fn run_agent_process_turn_with_cancellation(
    turn: impl std::future::Future<Output = Result<AgentProcessOutput>>,
    session_cancellation: Option<watch::Receiver<bool>>,
    external_cancellation: Option<watch::Receiver<bool>>,
) -> Result<AgentProcessOutput> {
    let turn = timeout(AGENT_PROCESS_TIMEOUT, turn);

    if session_cancellation.is_some() || external_cancellation.is_some() {
        tokio::select! {
            result = turn => {
                result.context("Codex app-server turn exceeded the automation timeout")?
            }
            _ = wait_for_any_cancellation(session_cancellation, external_cancellation) => {
                Err(report!(AutomationCancelled).into_dynamic())
            }
        }
    } else {
        turn.await
            .context("Codex app-server turn exceeded the automation timeout")?
    }
}

async fn wait_for_any_cancellation(
    session_cancellation: Option<watch::Receiver<bool>>,
    external_cancellation: Option<watch::Receiver<bool>>,
) {
    match (session_cancellation, external_cancellation) {
        (Some(mut session_cancellation), Some(mut external_cancellation)) => {
            tokio::select! {
                _ = wait_for_cancellation(&mut session_cancellation) => {}
                _ = wait_for_cancellation(&mut external_cancellation) => {}
            }
        }
        (Some(mut cancellation), None) | (None, Some(mut cancellation)) => {
            wait_for_cancellation(&mut cancellation).await;
        }
        (None, None) => {}
    }
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    loop {
        if *cancellation.borrow() {
            break;
        }
        if cancellation.changed().await.is_err() {
            break;
        }
    }
}

async fn run_codex_app_server_turn(
    start: AgentProcessStart,
    sessions: Option<ProcessSessionRegistry>,
) -> Result<AgentProcessOutput> {
    let prompt = tokio::fs::read_to_string(&start.prompt_path)
        .await
        .context_with(|| format!("failed to read prompt {}", start.prompt_path.display()))?;
    let working_dir = start.working_dir.to_string_lossy().into_owned();
    let mut output = Vec::new();

    push_codex_output_piece(
        &sessions,
        start.run_id,
        &mut output,
        OutputPieceDraft {
            kind: AgentRunOutputKind::System,
            item_id: None,
            title: "Codex app-server".to_owned(),
            body: format!(
                "starting Codex app-server from {}",
                start.codex_binary.display()
            ),
            metadata: serde_json::json!({
                "codex_binary": start.codex_binary.to_string_lossy(),
            }),
        },
    )
    .await;

    let env = agent_environment(
        &start.patchbay_binary,
        &GitRuntimeFiles {
            shim_dir: start.git_shim_dir.clone(),
            policy_path: start.git_policy_path.clone(),
        },
        &start.real_git_path,
        &start.project_name,
        &start.agent_id,
        start.claimed_item_id,
        SERVER_API_URL.get().map(String::as_str),
    );
    let mut thread_options = ThreadOptions::builder()
        .working_directory(working_dir)
        .sandbox_mode(agent_sandbox_mode_for_run(
            start.mutability,
            start.agent_sandbox_mode,
        ))
        .approval_policy(ApprovalMode::Never)
        .network_access_enabled(true)
        .sandbox_policy(agent_sandbox_policy_for_run(
            start.mutability,
            start.agent_sandbox_mode,
            &start.agent_extra_writable_roots,
        ))
        .config(codex_memory_config_overrides());
    if let Some(agent_model) = start.agent_model.as_deref() {
        thread_options = thread_options.model(agent_model);
    }
    if let Some(agent_reasoning_effort) = start.agent_reasoning_effort {
        thread_options =
            thread_options.model_reasoning_effort(to_codex_reasoning(agent_reasoning_effort));
    }
    let thread_options = thread_options.build();
    let (mut thread, mut streamed) =
        start_codex_streamed_turn(&start, &env, &thread_options, None, prompt)
            .await
            .map_err(|err| report!(err))
            .context("failed to start Codex app-server turn")?;
    let mut thread_id = thread.id().map(ToOwned::to_owned);

    let mut final_answer = None;
    let mut fallback_answer = None;
    let mut recovery_attempts = 0;
    let token_usage = loop {
        let event = match streamed.next_event().await {
            Some(Ok(event)) => event,
            Some(Err(err)) => {
                let resumed = recover_codex_streamed_turn(
                    CodexStreamRecoveryContext {
                        start: &start,
                        sessions: &sessions,
                        env: &env,
                        thread_options: &thread_options,
                        output: &mut output,
                    },
                    thread_id.as_deref(),
                    &mut recovery_attempts,
                    err,
                )
                .await?;
                thread = resumed.0;
                streamed = resumed.1;
                thread_id = thread.id().map(ToOwned::to_owned).or(thread_id);
                continue;
            }
            None => {
                let resumed = recover_codex_streamed_turn(
                    CodexStreamRecoveryContext {
                        start: &start,
                        sessions: &sessions,
                        env: &env,
                        thread_options: &thread_options,
                        output: &mut output,
                    },
                    thread_id.as_deref(),
                    &mut recovery_attempts,
                    ClientError::TransportClosed,
                )
                .await?;
                thread = resumed.0;
                streamed = resumed.1;
                thread_id = thread.id().map(ToOwned::to_owned).or(thread_id);
                continue;
            }
        };
        if let Some(piece) = thread_event_output_piece(&event) {
            push_codex_output_piece(&sessions, start.run_id, &mut output, piece).await;
        }

        match &event {
            ThreadEvent::ThreadStarted {
                thread_id: started_thread_id,
            } => {
                thread_id = Some(started_thread_id.clone());
            }
            ThreadEvent::ItemCompleted { item } => {
                update_response_candidates(item, &mut final_answer, &mut fallback_answer);
            }
            ThreadEvent::TurnCompleted { usage } => {
                break usage.as_ref().map(|usage| AgentRunTokenUsageView {
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage.cached_input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.input_tokens.saturating_add(usage.output_tokens),
                });
            }
            ThreadEvent::TurnFailed { error } => {
                bail!("Codex app-server turn failed: {}", error.message);
            }
            ThreadEvent::Error { message } => {
                bail!("Codex app-server stream error: {message}");
            }
            ThreadEvent::TurnStarted
            | ThreadEvent::ItemStarted { .. }
            | ThreadEvent::ItemUpdated { .. } => {}
        }
    };

    Ok(AgentProcessOutput {
        process_id: None,
        output,
        final_response: final_answer.or(fallback_answer).unwrap_or_default(),
        token_usage,
    })
}

async fn start_codex_streamed_turn(
    start: &AgentProcessStart,
    env: &HashMap<String, String>,
    thread_options: &ThreadOptions,
    thread_id: Option<&str>,
    input: impl Into<String>,
) -> std::result::Result<(Thread, StreamedTurn), CodexStreamStartError> {
    let codex = codex_app_server::spawn_codex_with_home_and_env(
        &start.codex_binary,
        &start.codex_home,
        env.clone(),
    )
    .await
    .map_err(CodexStreamStartError::Spawn)?;
    let mut thread = if let Some(thread_id) = thread_id {
        codex.resume_thread_by_id(thread_id.to_owned(), thread_options.clone())
    } else {
        codex.start_thread(thread_options.clone())
    };
    let streamed = thread
        .run_streamed(input.into(), TurnOptions::default())
        .await
        .map_err(CodexStreamStartError::Run)?;
    Ok((thread, streamed))
}

async fn recover_codex_streamed_turn(
    context: CodexStreamRecoveryContext<'_>,
    thread_id: Option<&str>,
    recovery_attempts: &mut usize,
    err: ClientError,
) -> Result<(Thread, StreamedTurn)> {
    let Some(reason) = recoverable_codex_stream_error_reason(&err) else {
        return Err(report!(err)
            .context("Codex app-server stream failed")
            .into_dynamic());
    };
    let Some(thread_id) = thread_id else {
        return Err(report!(err)
            .context("Codex app-server stream failed before a resumable thread id was available")
            .into_dynamic());
    };
    if *recovery_attempts >= CODEX_STREAM_RECOVERY_MAX_ATTEMPTS {
        bail!(
            "Codex app-server stream failed after {} recovery attempt(s): {err}",
            CODEX_STREAM_RECOVERY_MAX_ATTEMPTS
        );
    }

    loop {
        *recovery_attempts += 1;
        let attempt = *recovery_attempts;
        let backoff = codex_stream_recovery_backoff(attempt);
        push_codex_output_piece(
            context.sessions,
            context.start.run_id,
            context.output,
            codex_stream_recovery_piece(thread_id, attempt, reason, &err, backoff),
        )
        .await;
        tokio::time::sleep(backoff).await;

        match start_codex_streamed_turn(
            context.start,
            context.env,
            context.thread_options,
            Some(thread_id),
            CODEX_STREAM_RECOVERY_PROMPT,
        )
        .await
        {
            Ok(resumed) => {
                push_codex_output_piece(
                    context.sessions,
                    context.start.run_id,
                    context.output,
                    OutputPieceDraft {
                        kind: AgentRunOutputKind::System,
                        item_id: None,
                        title: "stream recovery resumed".to_owned(),
                        body: format!("resumed Codex thread {thread_id} after reconnect"),
                        metadata: serde_json::json!({
                            "thread_id": thread_id,
                            "recovery_attempt": attempt,
                            "recoverable": true,
                        }),
                    },
                )
                .await;
                return Ok(resumed);
            }
            Err(start_err) if recoverable_codex_stream_start_error(&start_err) => {
                if *recovery_attempts >= CODEX_STREAM_RECOVERY_MAX_ATTEMPTS {
                    return Err(report!(start_err)
                        .context(format!(
                            "Codex app-server stream did not recover after {} attempt(s)",
                            CODEX_STREAM_RECOVERY_MAX_ATTEMPTS
                        ))
                        .into_dynamic());
                }
                push_codex_output_piece(
                    context.sessions,
                    context.start.run_id,
                    context.output,
                    OutputPieceDraft {
                        kind: AgentRunOutputKind::System,
                        item_id: None,
                        title: "stream recovery retry".to_owned(),
                        body: format!(
                            "reconnect attempt {attempt} did not resume yet: {start_err}"
                        ),
                        metadata: serde_json::json!({
                            "thread_id": thread_id,
                            "recovery_attempt": attempt,
                            "max_recovery_attempts": CODEX_STREAM_RECOVERY_MAX_ATTEMPTS,
                            "recoverable": true,
                            "error": start_err.to_string(),
                        }),
                    },
                )
                .await;
            }
            Err(start_err) => {
                return Err(report!(start_err)
                    .context("Codex app-server stream recovery failed with a non-retryable error")
                    .into_dynamic());
            }
        }
    }
}

fn codex_stream_recovery_piece(
    thread_id: &str,
    attempt: usize,
    reason: &'static str,
    err: &ClientError,
    backoff: Duration,
) -> OutputPieceDraft {
    OutputPieceDraft {
        kind: AgentRunOutputKind::System,
        item_id: None,
        title: "recoverable stream interruption".to_owned(),
        body: format!(
            "Codex app-server stream interrupted ({reason}); reconnect attempt {attempt}/{} in {}s",
            CODEX_STREAM_RECOVERY_MAX_ATTEMPTS,
            backoff.as_secs()
        ),
        metadata: serde_json::json!({
            "thread_id": thread_id,
            "recovery_attempt": attempt,
            "max_recovery_attempts": CODEX_STREAM_RECOVERY_MAX_ATTEMPTS,
            "reason": reason,
            "recoverable": true,
            "error": err.to_string(),
        }),
    }
}

fn recoverable_codex_stream_start_error(err: &CodexStreamStartError) -> bool {
    match err {
        CodexStreamStartError::Spawn(_) => false,
        CodexStreamStartError::Run(err) => recoverable_codex_stream_error_reason(err).is_some(),
    }
}

fn recoverable_codex_stream_error_reason(err: &ClientError) -> Option<&'static str> {
    match err {
        ClientError::TransportClosed => Some("transport closed"),
        ClientError::TransportSend(message) if recoverable_transport_message(message) => {
            Some("transport send failed")
        }
        ClientError::Io(err) if recoverable_transport_io_error(err.kind()) => {
            Some("transport I/O interrupted")
        }
        ClientError::Timeout { .. } => Some("request timed out"),
        ClientError::Rpc { error } if recoverable_rpc_message(&error.message) => {
            Some("turn still active during reconnect")
        }
        ClientError::NotInitialized { .. }
        | ClientError::NotReady { .. }
        | ClientError::AlreadyInitialized
        | ClientError::TransportSend(_)
        | ClientError::InvalidMessage(_)
        | ClientError::Serialization(_)
        | ClientError::Io(_)
        | ClientError::Rpc { .. }
        | ClientError::UnexpectedResult { .. } => None,
    }
}

fn recoverable_transport_io_error(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::BrokenPipe
            | ErrorKind::ConnectionAborted
            | ErrorKind::ConnectionReset
            | ErrorKind::Interrupted
            | ErrorKind::NotConnected
            | ErrorKind::TimedOut
            | ErrorKind::UnexpectedEof
    )
}

fn recoverable_transport_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "event channel receive failed",
        "failed to send outbound frame",
        "transport closed",
        "connection aborted",
        "connection closed",
        "connection lost",
        "connection reset",
        "broken pipe",
        "channel closed",
        "timed out",
        "timeout",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn recoverable_rpc_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "active turn",
        "turn already",
        "turn is already",
        "turn is still",
        "currently running",
        "in progress",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn codex_stream_recovery_backoff(attempt: usize) -> Duration {
    let seconds = match attempt {
        0 | 1 => 2,
        2 => 5,
        3 => 10,
        4 => 20,
        _ => 30,
    };
    Duration::from_secs(seconds)
}

async fn create_pull_request(working_dir: &Path) -> Result<String> {
    let working_dir = working_dir.to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        StdCommand::new("gh")
            .arg("pr")
            .arg("create")
            .arg("--fill")
            .current_dir(working_dir)
            .output()
    })
    .await
    .context("PR creation task failed")?
    .context("failed to start gh pr create")?;
    if !output.status.success() {
        bail!(
            "gh pr create failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn update_run_pr_url(
    store: &Store,
    run: AgentRunModel,
    pr_url: Option<String>,
) -> Result<AgentRunModel> {
    let mut active: AgentRunActiveModel = run.into();
    active.pr_url = Set(pr_url);
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update run PR URL")?;
    publish_run_model_event(store, &updated).await;
    Ok(updated)
}

async fn cleanup_worktree_for_run(
    store: &Store,
    run: AgentRunModel,
    repo_path: &Path,
) -> Result<AgentRunModel> {
    if run.status == AgentRunStatus::Running.as_storage() {
        return Ok(run);
    }
    let Some(worktree_path) = run.worktree_path.clone() else {
        return update_run_cleanup(store, run, "not_applicable", None).await;
    };
    if run.cleanup_status == "cleaned" {
        return Ok(run);
    }
    let branch_name = run
        .branch_name
        .clone()
        .ok_or_else(|| report!("run {} has a worktree but no branch name", run.id))?;
    prune_git_worktree(repo_path, &branch_name, Path::new(&worktree_path))?;
    update_run_cleanup(store, run, "cleaned", Some(utc_now())).await
}

async fn update_run_cleanup(
    store: &Store,
    run: AgentRunModel,
    cleanup_status: &str,
    worktree_cleaned_at: Option<String>,
) -> Result<AgentRunModel> {
    let mut active: AgentRunActiveModel = run.into();
    active.cleanup_status = Set(cleanup_status.to_owned());
    active.worktree_cleaned_at = Set(worktree_cleaned_at);
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update worktree cleanup status")?;
    publish_run_model_event(store, &updated).await;
    Ok(updated)
}

fn prune_git_worktree(repo_path: &Path, branch_name: &str, worktree_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path)
        .context_with(|| format!("failed to open git repository '{}'", repo_path.display()))?;
    match repo.find_worktree(&worktree_name(branch_name)) {
        Ok(worktree) => {
            let mut prune_options = WorktreePruneOptions::new();
            prune_options.valid(true).working_tree(true);
            worktree.prune(Some(&mut prune_options)).context_with(|| {
                format!("failed to prune git worktree '{}'", worktree_path.display())
            })?;
        }
        Err(err) => {
            if !worktree_path.exists() {
                return Ok(());
            }
            fs::remove_dir_all(worktree_path).context_with(|| {
                format!(
                    "failed to remove stale worktree directory '{}' after git lookup failed: {err}",
                    worktree_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn ensure_git_worktree_clean(path: &Path) -> Result<()> {
    if !git_worktree_is_clean(path)? {
        bail!(
            "current workspace '{}' has uncommitted changes",
            path.display()
        );
    }
    Ok(())
}

fn git_worktree_is_clean(path: &Path) -> Result<bool> {
    let repo = Repository::open(path)
        .context_with(|| format!("failed to open git repository '{}'", path.display()))?;
    let mut status_options = StatusOptions::new();
    status_options
        .include_untracked(true)
        .recurse_untracked_dirs(true);
    let statuses = repo
        .statuses(Some(&mut status_options))
        .context_with(|| format!("failed to read git status for '{}'", path.display()))?;
    Ok(statuses.is_empty())
}

fn create_and_checkout_git_branch(repo_path: &Path, branch_name: &str) -> Result<()> {
    let repo = Repository::open(repo_path)
        .context_with(|| format!("failed to open git repository '{}'", repo_path.display()))?;
    ensure_git_worktree_clean(repo_path)?;
    let head = repo.head().context("failed to read repository HEAD")?;
    let target = head
        .peel_to_commit()
        .context("repository HEAD does not point to a commit")?;
    repo.branch(branch_name, &target, false)
        .context_with(|| format!("failed to create branch '{branch_name}'"))?;
    repo.set_head(&format!("refs/heads/{branch_name}"))
        .context_with(|| format!("failed to set HEAD to '{branch_name}'"))?;
    let mut checkout = CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_head(Some(&mut checkout))
        .context_with(|| format!("failed to check out branch '{branch_name}'"))?;
    Ok(())
}

fn create_git_worktree(repo_path: &Path, branch_name: &str, worktree_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path)
        .context_with(|| format!("failed to open git repository '{}'", repo_path.display()))?;
    let head = repo.head().context("failed to read repository HEAD")?;
    let target = head
        .peel_to_commit()
        .context("repository HEAD does not point to a commit")?;
    repo.branch(branch_name, &target, false)
        .context_with(|| format!("failed to create branch '{branch_name}'"))?;
    let branch_reference = repo
        .find_reference(&format!("refs/heads/{branch_name}"))
        .context_with(|| format!("failed to read branch reference '{branch_name}'"))?;
    let mut options = WorktreeAddOptions::new();
    options.reference(Some(&branch_reference));
    repo.worktree(
        worktree_name(branch_name).as_str(),
        worktree_path,
        Some(&options),
    )
    .context_with(|| format!("failed to create worktree '{}'", worktree_path.display()))?;
    Ok(())
}

fn worktree_name(branch_name: &str) -> String {
    branch_name.replace('/', "-")
}

fn automation_log_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".patchbay").join("runs");
    }
    PathBuf::from(".patchbay").join("runs")
}

async fn read_optional_text(path: Option<&str>) -> Result<Option<String>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let body = match tokio::fs::read_to_string(path).await {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context_with(|| format!("failed to read {}", path))?,
    };
    Ok(Some(body))
}

async fn model_to_view_with_log_usage(run: AgentRunModel) -> Result<AgentRunView> {
    let log_path = run.log_path.clone();
    let mut view = model_to_view(run)?;
    if view.token_usage.is_none() {
        view.token_usage = read_run_token_usage(log_path.as_deref()).await;
    }
    Ok(view)
}

fn summarize_text(value: &str) -> String {
    let mut summary = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned();
    if summary.len() > 4000 {
        let mut end = 4000;
        while end > 0 && !summary.is_char_boundary(end) {
            end -= 1;
        }
        summary.truncate(end);
        summary.push_str("...");
    }
    summary
}

fn slugify(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if slug.is_empty() {
        "project".to_owned()
    } else {
        slug
    }
}

fn model_to_view(run: AgentRunModel) -> Result<AgentRunView> {
    Ok(AgentRunView {
        id: run.id,
        project_id: run.project_id,
        work_item_id: run.work_item_id,
        memory_event_id: run.memory_event_id,
        trigger_id: run.trigger_id,
        trigger_name: projects::normalize_optional(run.trigger_name),
        tool_name: AgentToolName::from_str(&run.tool_name)?,
        mutability: AutomationRunMutability::from_str(&run.mutability)?,
        status: AgentRunStatus::from_str(&run.status)?,
        command: run.command,
        working_dir: run.working_dir,
        worktree_path: run.worktree_path,
        branch_name: run.branch_name,
        process_id: run.process_id,
        exit_code: run.exit_code,
        log_path: run.log_path,
        prompt_path: run.prompt_path,
        agent_model: projects::normalize_optional(run.agent_model),
        agent_reasoning_effort: run
            .agent_reasoning_effort
            .as_deref()
            .map(str::parse::<AgentReasoningEffort>)
            .transpose()?,
        token_usage: token_usage_from_columns(
            run.input_tokens,
            run.cached_input_tokens,
            run.output_tokens,
        ),
        commit_required: run.commit_required,
        commit_outcome: AgentCommitOutcome::from_str(&run.commit_outcome)?,
        commit_shas: parse_commit_shas(&run.commit_shas)?,
        pr_requested: run.pr_requested,
        pr_url: run.pr_url,
        cleanup_status: run.cleanup_status,
        worktree_cleaned_at: run.worktree_cleaned_at,
        result_summary: run.result_summary,
        started_at: run.started_at,
        finished_at: run.finished_at,
        created_at: run.created_at,
        updated_at: run.updated_at,
    })
}

fn token_usage_from_columns(
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
) -> Option<AgentRunTokenUsageView> {
    let input_tokens = input_tokens?;
    let cached_input_tokens = cached_input_tokens.unwrap_or_default();
    let output_tokens = output_tokens?;
    Some(AgentRunTokenUsageView {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        total_tokens: input_tokens.saturating_add(output_tokens),
    })
}

fn parse_commit_shas(raw: &str) -> Result<Vec<String>> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(raw).context("failed to decode automation commit SHAs")?)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        agent_ids,
        item_claims::claim_item,
        items::{CreateWorkItem, create_item, get_item},
        projects::{
            CreateProject, UpdateProjectSettings, create_project, get_project, get_settings,
            update_settings,
        },
    };
    use crate::shared::view_models::AUTOMATION_BLOCKED_LABEL_KEY;

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
                system_prompt: Some("Prefer concise automation.".to_owned()),
                memory: Some("Use Patchbay comments.".to_owned()),
            },
        )
        .await
        .unwrap();
        (temp, store)
    }

    #[test]
    fn dev_patchbay_cli_search_walks_up_to_repo_root() {
        let temp = TempDir::new().unwrap();
        let shim = temp.path().join("dev-bin/patchbay");
        fs::create_dir_all(shim.parent().unwrap()).unwrap();
        fs::write(&shim, "#!/usr/bin/env sh\n").unwrap();
        let server_workdir = temp.path().join("patchbay-server/target/debug");
        fs::create_dir_all(&server_workdir).unwrap();

        let search = find_dev_patchbay_cli_from_roots([server_workdir]);

        assert_eq!(search.path.as_deref(), Some(shim.as_path()));
    }

    #[test]
    fn current_branch_accepts_non_git_directory() {
        let temp = TempDir::new().unwrap();

        let plan = prepare_workspace(1, "demo", temp.path(), WorkspaceMode::CurrentBranch).unwrap();

        assert_eq!(plan.working_dir, temp.path());
        assert!(plan.worktree_path.is_none());
        assert!(plan.branch_name.is_none());
    }

    #[test]
    fn current_branch_accepts_dirty_unborn_git_repository() {
        let temp = TempDir::new().unwrap();
        Repository::init(temp.path()).unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\n",
        )
        .unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();

        let plan = prepare_workspace(1, "demo", temp.path(), WorkspaceMode::CurrentBranch).unwrap();

        assert_eq!(plan.working_dir, temp.path());
        assert!(plan.worktree_path.is_none());
        assert!(plan.branch_name.is_none());
    }

    #[test]
    fn read_only_workspace_uses_project_checkout_without_branch_or_worktree() {
        let temp = TempDir::new().unwrap();

        let plan = prepare_workspace_for_run(
            1,
            "demo",
            temp.path(),
            WorkspaceMode::GitWorktree,
            AutomationRunMutability::ReadOnly,
        )
        .unwrap();

        assert_eq!(plan.working_dir, temp.path());
        assert!(plan.worktree_path.is_none());
        assert!(plan.branch_name.is_none());
    }

    #[tokio::test]
    async fn run_log_uses_active_session_output_when_available() {
        let (temp, store) = test_store().await;
        let project = get_project(&store, "demo").await.unwrap();
        let run = create_run(
            &store,
            project.id,
            AgentToolName::Codex,
            AutomationRunMutability::Mutating,
            None,
        )
        .await
        .unwrap();
        let log_path = temp.path().join("run.output.json");
        write_run_output_log(
            &log_path,
            &[new_output_piece(
                1,
                AgentRunOutputKind::ModelMessage,
                None,
                "persisted",
                "persisted output",
                serde_json::json!({}),
            )],
        )
        .unwrap();
        let run = update_run_launch_details(
            &store,
            run,
            LaunchDetails {
                work_item_id: None,
                command: "codex app-server turn prompt.md".to_owned(),
                workspace: WorkspacePlan {
                    working_dir: temp.path().to_path_buf(),
                    worktree_path: None,
                    branch_name: None,
                },
                prompt_path: None,
                log_path: Some(log_path.to_string_lossy().into_owned()),
                memory_event_id: None,
                agent_model: None,
                agent_reasoning_effort: None,
                commit_required: false,
                pr_requested: false,
            },
        )
        .await
        .unwrap();
        let sessions = ProcessSessionRegistry::new();
        let _cancel = sessions
            .begin(ProcessSessionStart {
                run_id: run.id,
                project_name: "demo".to_owned(),
                tool_name: "codex".to_owned(),
                command: "codex app-server turn prompt.md".to_owned(),
                working_dir: temp.path().to_string_lossy().into_owned(),
            })
            .await;
        sessions
            .append_output_piece(
                run.id,
                new_output_piece(
                    1,
                    AgentRunOutputKind::ModelMessage,
                    None,
                    "active",
                    "active output",
                    serde_json::json!({}),
                ),
            )
            .await;

        let run_log = read_run_log_with_active_session(&store, &sessions, "demo", run.id)
            .await
            .unwrap();

        assert!(run_log.active);
        assert_eq!(run_log.output.len(), 1);
        assert_eq!(run_log.output[0].title, "active");
        assert_eq!(run_log.output[0].body, "active output");
    }

    #[tokio::test]
    async fn mutating_and_read_only_runs_have_independent_admission_limits() {
        let (_temp, store) = test_store().await;
        let project = get_project(&store, "demo").await.unwrap();

        let mutating = create_run(
            &store,
            project.id,
            AgentToolName::Codex,
            AutomationRunMutability::Mutating,
            None,
        )
        .await
        .unwrap();
        let read_only = create_run(
            &store,
            project.id,
            AgentToolName::Codex,
            AutomationRunMutability::ReadOnly,
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            model_to_view(mutating).unwrap().mutability,
            AutomationRunMutability::Mutating
        );
        assert_eq!(
            model_to_view(read_only).unwrap().mutability,
            AutomationRunMutability::ReadOnly
        );
        assert!(
            !can_start_automation_run(&store, "demo", AutomationRunMutability::Mutating)
                .await
                .unwrap()
        );
        let settings = get_settings(&store, "demo").await.unwrap();
        let err = enforce_concurrency(&store, "demo", &settings, AutomationRunMutability::Mutating)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("mutating"));
        assert!(err.to_string().contains("limit is 1"));
        assert!(
            can_start_automation_run(&store, "demo", AutomationRunMutability::ReadOnly)
                .await
                .unwrap()
        );

        let status = automation_status(&store, "demo").await.unwrap();
        assert_eq!(status.running_runs, 2);
        assert_eq!(status.running_mutating_runs, 1);
        assert_eq!(status.running_read_only_runs, 1);
        assert_eq!(status.allowed_mutating_runs, 1);
        assert_eq!(status.settings.max_read_only_agents, 2);

        create_run(
            &store,
            project.id,
            AgentToolName::Codex,
            AutomationRunMutability::ReadOnly,
            None,
        )
        .await
        .unwrap();
        assert!(
            !can_start_automation_run(&store, "demo", AutomationRunMutability::ReadOnly)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn read_only_admission_can_be_disabled_with_zero_limit() {
        let (_temp, store) = test_store().await;
        let settings = update_settings(
            &store,
            "demo",
            UpdateProjectSettings {
                max_read_only_agents: Some(0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert!(
            !can_start_automation_run(&store, "demo", AutomationRunMutability::ReadOnly)
                .await
                .unwrap()
        );
        let err = enforce_concurrency(&store, "demo", &settings, AutomationRunMutability::ReadOnly)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("read-only"));
        assert!(err.to_string().contains("limit is 0"));
    }

    #[tokio::test]
    async fn effective_agent_settings_prefer_item_overrides() {
        let (_temp, store) = test_store().await;
        let settings = update_settings(
            &store,
            "demo",
            UpdateProjectSettings {
                default_agent_model: Some(Some("gpt-5.5".to_owned())),
                default_agent_reasoning_effort: Some(Some(AgentReasoningEffort::High)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Configured item".to_owned(),
                description: "Exercise item overrides".to_owned(),
                state: "open".to_owned(),
                agent_model_override: Some("gpt-5-codex".to_owned()),
                agent_reasoning_effort_override: Some(AgentReasoningEffort::Medium),
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            effective_agent_model(&settings, Some(&item)).as_deref(),
            Some("gpt-5-codex")
        );
        assert_eq!(
            effective_agent_reasoning_effort(&settings, Some(&item)),
            Some(AgentReasoningEffort::Medium)
        );
    }

    #[test]
    fn agent_environment_exposes_api_but_not_database() {
        let git_runtime = GitRuntimeFiles {
            shim_dir: PathBuf::from("/tmp/patchbay-run-bin"),
            policy_path: PathBuf::from("/tmp/patchbay-git-policy.json"),
        };
        let env = agent_environment(
            Path::new("/tmp/patchbay"),
            &git_runtime,
            Path::new("/usr/bin/git"),
            "demo",
            "patchbay-run-1",
            Some(42),
            Some("http://127.0.0.1:4000"),
        );

        assert_eq!(
            env.get("PATCHBAY_PROJECT").map(String::as_str),
            Some("demo")
        );
        assert_eq!(
            env.get("PATCHBAY_AGENT_ID").map(String::as_str),
            Some("patchbay-run-1")
        );
        assert_eq!(
            env.get("PATCHBAY_CLAIMED_ITEM_ID").map(String::as_str),
            Some("42")
        );
        assert_eq!(
            env.get("PATCHBAY_API_URL").map(String::as_str),
            Some("http://127.0.0.1:4000")
        );
        assert_eq!(
            env.get("PATCHBAY_GIT_POLICY_PATH").map(String::as_str),
            Some("/tmp/patchbay-git-policy.json")
        );
        assert_eq!(
            env.get("PATCHBAY_REAL_GIT").map(String::as_str),
            Some("/usr/bin/git")
        );
        assert!(
            env.get("PATH")
                .is_some_and(|path| path.starts_with("/tmp/patchbay-run-bin:"))
        );
        assert!(!env.contains_key("PATCHBAY_DATABASE"));
        assert!(!env.contains_key("PATCHBAY_URL"));
    }

    #[test]
    fn codex_thread_config_disables_internal_memory() {
        let config = codex_memory_config_overrides();

        assert_eq!(
            config.get("features.memories"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(
            config.get("memories.use_memories"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(
            config.get("memories.generate_memories"),
            Some(&serde_json::Value::Bool(false))
        );
    }

    #[test]
    fn codex_stream_recovery_classifies_transport_interruptions() {
        assert_eq!(
            recoverable_codex_stream_error_reason(&ClientError::TransportClosed),
            Some("transport closed")
        );
        assert_eq!(
            recoverable_codex_stream_error_reason(&ClientError::TransportSend(
                "event channel receive failed: channel closed".to_owned()
            )),
            Some("transport send failed")
        );
        assert_eq!(
            recoverable_codex_stream_error_reason(&ClientError::Io(std::io::Error::from(
                ErrorKind::BrokenPipe
            ))),
            Some("transport I/O interrupted")
        );
        assert_eq!(
            recoverable_codex_stream_error_reason(&ClientError::Timeout {
                method: "turn/start".to_owned(),
                timeout_ms: 30_000,
            }),
            Some("request timed out")
        );
    }

    #[test]
    fn codex_stream_recovery_leaves_non_retryable_errors_terminal() {
        assert_eq!(
            recoverable_codex_stream_error_reason(&ClientError::TransportSend(
                "thread id unavailable after start/resume".to_owned()
            )),
            None
        );
        assert_eq!(
            recoverable_codex_stream_error_reason(&ClientError::InvalidMessage(
                "expected JSON object".to_owned()
            )),
            None
        );
        assert_eq!(
            recoverable_codex_stream_error_reason(&ClientError::Rpc {
                error: codex_app_server_sdk::RpcError {
                    code: -32_000,
                    message: "model rejected the request".to_owned(),
                    data: None,
                },
            }),
            None
        );
    }

    #[test]
    fn codex_stream_recovery_logs_concise_system_note() {
        let err = ClientError::TransportClosed;
        let piece = codex_stream_recovery_piece(
            "thread-1",
            1,
            "transport closed",
            &err,
            Duration::from_secs(2),
        );

        assert_eq!(piece.kind, AgentRunOutputKind::System);
        assert_eq!(piece.title, "recoverable stream interruption");
        assert!(piece.body.contains("reconnect attempt 1/"));
        assert_eq!(piece.metadata["recoverable"], true);
        assert_eq!(piece.metadata["thread_id"], "thread-1");
    }

    #[tokio::test]
    async fn explicit_cancellation_still_cancels_waiting_turn() {
        let (cancel, cancellation) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(run_agent_process_turn_with_cancellation(
            std::future::pending::<Result<AgentProcessOutput>>(),
            None,
            Some(cancellation),
        ));

        cancel.send(true).unwrap();
        let err = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();

        assert!(is_automation_cancelled(&err));
    }

    #[tokio::test]
    async fn non_retryable_turn_error_still_fails() {
        let err = run_agent_process_turn_with_cancellation(
            async { bail!("permanent Codex SDK failure") },
            None,
            None,
        )
        .await
        .unwrap_err();

        assert!(!is_automation_cancelled(&err));
        assert!(err.to_string().contains("permanent Codex SDK failure"));
    }

    #[test]
    fn codex_thread_sandbox_uses_project_writable_roots() {
        let roots = vec![
            "/tmp/patchbay-browser".to_owned(),
            "/Users/test/.patchbay/codex".to_owned(),
        ];
        let policy = agent_sandbox_policy(AgentSandboxMode::WorkspaceWrite, &roots);

        assert_eq!(
            policy,
            serde_json::json!({
                "type": "workspaceWrite",
                "networkAccess": true,
                "writableRoots": roots,
            })
        );
    }

    #[test]
    fn codex_thread_sandbox_can_disable_sandbox_for_project() {
        let roots = vec!["/tmp/ignored-when-full-access".to_owned()];

        assert_eq!(
            agent_sandbox_mode(AgentSandboxMode::DangerFullAccess),
            SandboxMode::DangerFullAccess
        );
        assert_eq!(
            agent_sandbox_policy(AgentSandboxMode::DangerFullAccess, &roots),
            serde_json::json!({
                "type": "dangerFullAccess",
            })
        );
    }

    #[test]
    fn read_only_codex_thread_sandbox_ignores_project_writable_roots() {
        let roots = vec!["/tmp/ignored-for-read-only".to_owned()];

        assert_eq!(
            agent_sandbox_mode_for_run(
                AutomationRunMutability::ReadOnly,
                AgentSandboxMode::DangerFullAccess
            ),
            SandboxMode::ReadOnly
        );
        assert_eq!(
            agent_sandbox_policy_for_run(
                AutomationRunMutability::ReadOnly,
                AgentSandboxMode::WorkspaceWrite,
                &roots
            ),
            serde_json::json!({
                "type": "readOnly",
                "networkAccess": true,
            })
        );
    }

    #[tokio::test]
    async fn read_only_git_runtime_policy_disables_mutable_commands() {
        let (_temp, store) = test_store().await;
        let settings = update_settings(
            &store,
            "demo",
            UpdateProjectSettings {
                workspace_mode: Some(WorkspaceMode::GitWorktree),
                max_code_edit_agents: Some(2),
                agent_git_command_policy: Some(patchbay_types::AgentGitCommandPolicy {
                    add: true,
                    commit: true,
                    push: true,
                    reset: true,
                    hard_reset: AgentGitHardResetPolicy::IsolatedWorkspaces,
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let policy = git_runtime_policy_for_run(&settings, AutomationRunMutability::ReadOnly);

        assert_eq!(policy.workspace_mode, WorkspaceMode::CurrentBranch);
        assert!(!policy.policy.add);
        assert!(!policy.policy.commit);
        assert!(!policy.policy.push);
        assert!(!policy.policy.reset);
        assert_eq!(policy.policy.hard_reset, AgentGitHardResetPolicy::Never);
    }

    #[tokio::test]
    async fn stop_automation_releases_claimed_work_back_to_source_state() {
        let (temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Cancel me".to_owned(),
                description: "Exercise cancellation release".to_owned(),
                state: "ready".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let project = get_project(&store, "demo").await.unwrap();
        let run = create_run(
            &store,
            project.id,
            AgentToolName::Codex,
            AutomationRunMutability::Mutating,
            None,
        )
        .await
        .unwrap();
        let agent_id = agent_ids::patchbay_run_agent_id(run.id);
        claim_item(&store, "demo", &agent_id, "ready")
            .await
            .unwrap()
            .unwrap();
        update_run_launch_details(
            &store,
            run,
            LaunchDetails {
                work_item_id: Some(item.id),
                command: "codex app-server turn prompt.md".to_owned(),
                workspace: WorkspacePlan {
                    working_dir: temp.path().to_path_buf(),
                    worktree_path: None,
                    branch_name: None,
                },
                prompt_path: None,
                log_path: None,
                memory_event_id: None,
                agent_model: None,
                agent_reasoning_effort: None,
                commit_required: false,
                pr_requested: false,
            },
        )
        .await
        .unwrap();

        let cancelled = stop_automation(&store, "demo").await.unwrap();
        let item = get_item(&store, "demo", item.id).await.unwrap();

        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].status, AgentRunStatus::Cancelled);
        assert_eq!(item.state.as_deref(), Some("ready"));
        assert_eq!(item.claimed_by, None);
        assert!(
            item.labels
                .iter()
                .all(|label| label.key != AUTOMATION_BLOCKED_LABEL_KEY)
        );
    }
}
