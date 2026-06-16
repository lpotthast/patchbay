use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::{Path, PathBuf},
    process::Command as StdCommand,
    str::FromStr,
    sync::OnceLock,
    time::Duration,
};

use codex_app_server_sdk::{
    ApprovalMode, ModelReasoningEffort as CodexReasoningEffort, ReviewModeItem, SandboxMode,
    ThreadEvent, ThreadItem, ThreadOptions, TurnOptions,
};
use crudkit_core::condition::Condition;
use git2::{
    Repository, StatusOptions, WorktreeAddOptions, WorktreePruneOptions, build::CheckoutBuilder,
};
use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use tokio::{sync::watch, time::timeout};

use crate::{
    backend::{
        agent_tools, codex_app_server,
        entities::agent_run::{self, AgentRun, AgentRunActiveModel, AgentRunModel},
        events, items,
        process_sessions::{ProcessSessionRegistry, ProcessSessionStart},
        projects,
        storage::{Store, utc_now},
    },
    shared::view_models::{
        AUTOMATION_BLOCKED_LABEL_KEY, AgentReasoningEffort, AgentRunOutputKind, AgentRunOutputLog,
        AgentRunOutputPiece, AgentRunStatus, AgentRunView, AgentToolName, AutomationMode,
        AutomationStatusView, CLAIMED_FROM_STATE_LABEL_KEY, DEFAULT_STATE_LABEL,
        FINISHED_STATE_LABEL, ProjectMemoryEventRefView, ProjectSettingsView, RecoveredClaimView,
        RunLogView, WorkItemView, WorkspaceMode, WorktreeCleanupPolicy,
    },
};

const AGENT_PROCESS_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);
const MAX_AGENT_OUTPUT_BYTES: usize = 1024 * 1024;
static SERVER_API_URL: OnceLock<String> = OnceLock::new();
const PATCHBAY_AGENT_INSTRUCTIONS: &str = include_str!("../../../AGENT_INSTRUCTIONS.md");

fn patchbay_agent_instructions_body() -> &'static str {
    PATCHBAY_AGENT_INSTRUCTIONS
        .strip_prefix("# Patchbay Agent Instructions\n\n")
        .unwrap_or(PATCHBAY_AGENT_INSTRUCTIONS)
        .trim()
}

#[derive(Clone, Debug)]
pub struct StartAutomation {
    pub mode: AutomationMode,
    pub tool: Option<AgentToolName>,
    pub work_item_id: Option<i64>,
    pub work_item_selector: Option<Condition>,
    pub extra_prompt: Option<String>,
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

struct LaunchDetails {
    work_item_id: Option<i64>,
    command: String,
    workspace: WorkspacePlan,
    prompt_path: Option<String>,
    log_path: Option<String>,
    memory_event_id: Option<i64>,
    agent_model: Option<String>,
    agent_reasoning_effort: Option<AgentReasoningEffort>,
    pr_requested: bool,
}

struct PromptContext<'a> {
    project_name: &'a str,
    mode: AutomationMode,
    system_prompt: &'a str,
    memory: &'a str,
    memory_event_id: Option<i64>,
    item: Option<&'a WorkItemView>,
    agent_id: &'a str,
    extra_prompt: Option<&'a str>,
    create_pr: bool,
}

struct AgentProcessOutput {
    process_id: Option<i64>,
    output: Vec<AgentRunOutputPiece>,
    final_response: String,
}

struct AgentProcessStart {
    run_id: i64,
    project_name: String,
    tool_name: AgentToolName,
    codex_binary: PathBuf,
    patchbay_binary: PathBuf,
    prompt_path: PathBuf,
    working_dir: PathBuf,
    agent_id: String,
    claimed_item_id: Option<i64>,
    agent_model: Option<String>,
    agent_reasoning_effort: Option<AgentReasoningEffort>,
}

struct OutputPieceDraft {
    kind: AgentRunOutputKind,
    item_id: Option<String>,
    title: String,
    body: String,
    metadata: serde_json::Value,
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
    automation_disposition: items::ReleaseAutomationDisposition,
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
    enforce_concurrency(store, project_name, &settings).await?;

    let tool = start.tool.unwrap_or(settings.default_agent_tool);
    let run = create_run(store, project.id, start.mode, tool, start.trigger.as_ref()).await?;

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
    let agent_id = format!("patchbay-run-{}", run.id);

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

    let claimed_item = if start.mode.claims_work() {
        let claimed = if let Some(work_item_id) = start.work_item_id {
            match items::claim_specific_item(store, &project_name, work_item_id, &agent_id).await {
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
            match items::claim_item_matching_condition(store, &project_name, &agent_id, condition)
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
            match items::claim_item(store, &project_name, &agent_id, DEFAULT_STATE_LABEL).await {
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
    } else {
        None
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

    let workspace = match prepare_workspace(
        run.id,
        &project_name,
        &project_path,
        settings.workspace_mode,
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
                    automation_disposition: items::ReleaseAutomationDisposition::Claimable,
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
    let prompt = build_prompt(PromptContext {
        project_name: &project_name,
        mode: start.mode,
        system_prompt: &project.system_prompt,
        memory: &project.memory,
        memory_event_id,
        item: claimed_item.as_ref(),
        agent_id: &agent_id,
        extra_prompt: start.extra_prompt.as_deref(),
        create_pr: settings.create_pr,
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
            pr_requested: settings.create_pr,
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

    let output = run_agent_process(
        AgentProcessStart {
            run_id: run.id,
            project_name: project_name.clone(),
            tool_name: tool,
            codex_binary,
            patchbay_binary,
            prompt_path,
            working_dir: PathBuf::from(&run.working_dir),
            agent_id: agent_id.clone(),
            claimed_item_id: claimed_item.as_ref().map(|item| item.id),
            agent_model,
            agent_reasoning_effort,
        },
        sessions,
        cancellation,
    )
    .await;
    match output {
        Ok(output) => {
            run = update_run_process_id(store, run, output.process_id).await?;
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
            if success && settings.create_pr {
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
                    automation_disposition: items::ReleaseAutomationDisposition::Blocked,
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
                        items::ReleaseAutomationDisposition::Claimable
                    } else {
                        items::ReleaseAutomationDisposition::Blocked
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
            automation_disposition: items::ReleaseAutomationDisposition::Claimable,
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
            automation_disposition: items::ReleaseAutomationDisposition::Claimable,
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
        let agent_id = format!("patchbay-run-{}", run.id);
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
                automation_disposition: items::ReleaseAutomationDisposition::Claimable,
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
    let running_runs = running_run_count(store, project_name).await?;
    let recent_runs = list_runs(store, project_name, Some(10)).await?;
    let tools = agent_tools::list_tools(store).await?;

    Ok(AutomationStatusView {
        project: project_name.to_owned(),
        settings,
        running_runs,
        recent_runs,
        tools,
    })
}

pub async fn active_project_names(store: &Store) -> Result<Vec<String>> {
    let projects = projects::list_projects(store).await?;
    let mut active = Vec::new();
    for project in projects {
        if running_run_count(store, &project.name).await? > 0 {
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
    runs.into_iter().map(model_to_view).collect()
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
    runs.into_iter().map(model_to_view).collect()
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
    runs.into_iter().map(model_to_view).collect()
}

pub async fn get_run(store: &Store, project_name: &str, run_id: i64) -> Result<AgentRunView> {
    let project_id = projects::project_id(store, project_name).await?;
    let run = AgentRun::find_by_id(run_id)
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context("failed to load agent run")?
        .ok_or_else(|| report!("agent run {run_id} does not exist in this project"))?;
    model_to_view(run)
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
        memory_event,
        prompt,
        output,
    })
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
        cleaned.push(model_to_view(
            cleanup_worktree_for_run(store, run, &project_path).await?,
        )?);
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
    items::recover_stale_claims(store, project_name, minutes).await
}

pub async fn recover_configured_stale_claims(store: &Store) -> Result<Vec<RecoveredClaimView>> {
    let projects = projects::list_projects(store).await?;
    let mut recovered = Vec::new();
    for project in projects {
        let settings = projects::get_settings(store, &project.name).await?;
        if settings.stale_claim_minutes > 0 {
            recovered.extend(
                items::recover_stale_claims(store, &project.name, settings.stale_claim_minutes)
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
) -> Result<()> {
    if settings.create_pr && settings.workspace_mode == WorkspaceMode::CurrentBranch {
        bail!("pull requests can only be created for git_worktree or git_branch strategies");
    }
    let allowed = projects::allowed_code_edit_agents(settings);
    let running = running_run_count(store, project_name).await?;
    if running >= allowed {
        bail!("project already has {running} running agent run(s); limit is {allowed}");
    }
    Ok(())
}

pub async fn can_start_automation_run(store: &Store, project_name: &str) -> Result<bool> {
    let settings = projects::get_settings(store, project_name).await?;
    let allowed = projects::allowed_code_edit_agents(&settings);
    let running = running_run_count(store, project_name).await?;
    Ok(running < allowed)
}

async fn running_run_count(store: &Store, project_name: &str) -> Result<i64> {
    let project_id = projects::project_id(store, project_name).await?;
    let count = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .filter(agent_run::Column::Status.eq(AgentRunStatus::Running.as_storage()))
        .count(store.db().as_ref())
        .await
        .context("failed to count running agent runs")?;
    Ok(count as i64)
}

async fn create_run(
    store: &Store,
    project_id: i64,
    mode: AutomationMode,
    tool: AgentToolName,
    trigger: Option<&AutomationTriggerOrigin>,
) -> Result<AgentRunModel> {
    let now = utc_now();
    let run = AgentRunActiveModel {
        project_id: Set(project_id),
        work_item_id: Set(None),
        memory_event_id: Set(None),
        trigger_id: Set(trigger.map(|trigger| trigger.trigger_id)),
        trigger_name: Set(trigger.map(|trigger| trigger.trigger_name.clone())),
        mode: Set(mode.as_storage().to_owned()),
        tool_name: Set(tool.as_storage().to_owned()),
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
    items::release_item(
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

fn agent_environment(
    patchbay_binary: &Path,
    project_name: &str,
    agent_id: &str,
    claimed_item_id: Option<i64>,
    api_url: Option<&str>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if let Some(bin_dir) = patchbay_binary.parent() {
        let path = std::env::var("PATH").unwrap_or_default();
        env.insert(
            "PATH".to_owned(),
            format!("{}:{path}", bin_dir.to_string_lossy()),
        );
    }
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

fn agent_sandbox_policy() -> serde_json::Value {
    serde_json::json!({
        "type": "workspaceWrite",
        "networkAccess": true,
        "writableRoots": [],
    })
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
    let turn = timeout(
        AGENT_PROCESS_TIMEOUT,
        run_codex_app_server_turn(start, sessions),
    );

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
        &start.project_name,
        &start.agent_id,
        start.claimed_item_id,
        SERVER_API_URL.get().map(String::as_str),
    );
    let codex = codex_app_server::spawn_codex_with_env(&start.codex_binary, env).await?;
    let mut thread_options = ThreadOptions::builder()
        .working_directory(working_dir)
        .sandbox_mode(SandboxMode::WorkspaceWrite)
        .approval_policy(ApprovalMode::Never)
        .sandbox_policy(agent_sandbox_policy())
        .config(codex_memory_config_overrides());
    if let Some(agent_model) = start.agent_model {
        thread_options = thread_options.model(agent_model);
    }
    if let Some(agent_reasoning_effort) = start.agent_reasoning_effort {
        thread_options =
            thread_options.model_reasoning_effort(to_codex_reasoning(agent_reasoning_effort));
    }
    let thread_options = thread_options.build();
    let mut thread = codex.start_thread(thread_options);
    let mut streamed = thread
        .run_streamed(prompt, TurnOptions::default())
        .await
        .context("failed to start Codex app-server turn")?;

    let mut saw_terminal = false;
    let mut final_answer = None;
    let mut fallback_answer = None;
    while let Some(event) = streamed.next_event().await {
        let event = event.context("Codex app-server stream failed")?;
        if let Some(piece) = thread_event_output_piece(&event) {
            push_codex_output_piece(&sessions, start.run_id, &mut output, piece).await;
        }

        match &event {
            ThreadEvent::ItemCompleted { item } => {
                update_response_candidates(item, &mut final_answer, &mut fallback_answer);
            }
            ThreadEvent::TurnCompleted { .. } => {
                saw_terminal = true;
                break;
            }
            ThreadEvent::TurnFailed { error } => {
                bail!("Codex app-server turn failed: {}", error.message);
            }
            ThreadEvent::Error { message } => {
                bail!("Codex app-server stream error: {message}");
            }
            ThreadEvent::ThreadStarted { .. }
            | ThreadEvent::TurnStarted
            | ThreadEvent::ItemStarted { .. }
            | ThreadEvent::ItemUpdated { .. } => {}
        }
    }

    if !saw_terminal {
        bail!("Codex app-server stream ended before turn completion");
    }

    Ok(AgentProcessOutput {
        process_id: None,
        output,
        final_response: final_answer.or(fallback_answer).unwrap_or_default(),
    })
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

fn build_prompt(context: PromptContext<'_>) -> String {
    let mut prompt = format!(
        "# Patchbay Automation\n\nProject: {}\nMode: {}\nAgent id: {}\n\n",
        context.project_name, context.mode, context.agent_id
    );
    prompt.push_str("## Patchbay Agent Instructions\n\n");
    prompt.push_str(patchbay_agent_instructions_body());
    prompt.push_str("\n\n");
    if context.item.is_none() {
        prompt.push_str(
            "This run has no claimed item, so commands that require an item id must be given one explicitly.\n\n",
        );
    }
    if !context.system_prompt.trim().is_empty() {
        prompt.push_str("## Project System Prompt\n\n");
        prompt.push_str(context.system_prompt);
        prompt.push_str("\n\n");
    }
    prompt.push_str("## Project Memory\n\n");
    if let Some(memory_event_id) = context.memory_event_id {
        prompt.push_str(&format!("MemoryChanged event: #{memory_event_id}\n\n"));
    }
    if context.memory.trim().is_empty() {
        prompt.push_str("(empty)\n\n");
    } else {
        prompt.push_str(context.memory);
        prompt.push_str("\n\n");
    }
    if let Some(item) = context.item {
        prompt.push_str("## Claimed Work Item\n\n");
        let state = item.state.as_deref().unwrap_or("(none)");
        let claimed_from_state = claimed_from_state_label(item).unwrap_or(state);
        let labels = item
            .labels
            .iter()
            .map(|label| match label.value.as_deref() {
                Some(value) => format!("{}={value}", label.key),
                None => label.key.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        prompt.push_str(&format!(
            "Item: #{}\nTitle: {}\nState label: {}\nClaimed from state label: {}\nRelease behavior: `patchbay item release` restores the claimed-from state and adds `{}` so automation will not pick the item again until that label is removed.\nLabels: {}\nVersion: {}\n\n{}\n\n",
            item.id,
            item.title,
            state,
            claimed_from_state,
            AUTOMATION_BLOCKED_LABEL_KEY,
            if labels.is_empty() { "(none)" } else { &labels },
            item.version,
            item.description
        ));
    }
    if let Some(extra_prompt) = context
        .extra_prompt
        .filter(|value| !value.trim().is_empty())
    {
        prompt.push_str("## Trigger Prompt\n\n");
        prompt.push_str(extra_prompt);
        prompt.push_str("\n\n");
    }
    if context.create_pr {
        prompt.push_str(
            "## Pull Request\n\nCreate a pull request after the requested work is committed. \
             Patchbay will also attempt `gh pr create --fill` after your process exits.\n\n",
        );
    }
    prompt
}

fn claimed_from_state_label(item: &WorkItemView) -> Option<&str> {
    item.labels
        .iter()
        .find(|label| label.key == CLAIMED_FROM_STATE_LABEL_KEY)
        .and_then(|label| label.value.as_deref())
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

async fn read_run_output(path: Option<&str>) -> Result<Vec<AgentRunOutputPiece>> {
    let Some(body) = read_optional_text(path).await? else {
        return Ok(Vec::new());
    };
    if let Ok(log) = serde_json::from_str::<AgentRunOutputLog>(&body) {
        return Ok(log.pieces);
    }
    Ok(vec![new_output_piece(
        1,
        AgentRunOutputKind::Legacy,
        None,
        "legacy log",
        body,
        serde_json::json!({ "format": "plain_text" }),
    )])
}

fn write_run_output_log(path: &Path, pieces: &[AgentRunOutputPiece]) -> Result<()> {
    let log = AgentRunOutputLog {
        schema_version: 1,
        pieces: pieces.to_vec(),
    };
    let body = serde_json::to_string_pretty(&log).context("failed to encode automation output")?;
    Ok(fs::write(path, body).context_with(|| format!("failed to write {}", path.display()))?)
}

async fn push_codex_output_piece(
    sessions: &Option<ProcessSessionRegistry>,
    run_id: i64,
    output: &mut Vec<AgentRunOutputPiece>,
    draft: OutputPieceDraft,
) {
    let piece = new_output_piece(
        output.last().map(|piece| piece.sequence + 1).unwrap_or(1),
        draft.kind,
        draft.item_id,
        draft.title,
        draft.body,
        draft.metadata,
    );
    output.push(piece.clone());
    trim_output_pieces(output, MAX_AGENT_OUTPUT_BYTES);
    if let Some(registry) = sessions {
        registry.append_output_piece(run_id, piece).await;
    }
}

fn new_output_piece(
    sequence: u64,
    kind: AgentRunOutputKind,
    item_id: Option<String>,
    title: impl Into<String>,
    body: impl Into<String>,
    metadata: serde_json::Value,
) -> AgentRunOutputPiece {
    AgentRunOutputPiece {
        sequence,
        timestamp: utc_now(),
        kind,
        source: "codex".to_owned(),
        item_id,
        title: title.into(),
        body: body.into(),
        metadata,
    }
}

fn trim_output_pieces(pieces: &mut Vec<AgentRunOutputPiece>, max_bytes: usize) {
    while pieces.len() > 1 && output_pieces_size(pieces) > max_bytes {
        pieces.remove(0);
    }
}

fn output_pieces_size(pieces: &[AgentRunOutputPiece]) -> usize {
    pieces.iter().map(output_piece_size).sum()
}

fn output_piece_size(piece: &AgentRunOutputPiece) -> usize {
    piece.timestamp.len()
        + piece.source.len()
        + piece.item_id.as_deref().map(str::len).unwrap_or_default()
        + piece.title.len()
        + piece.body.len()
        + piece.metadata.to_string().len()
}

fn thread_event_output_piece(event: &ThreadEvent) -> Option<OutputPieceDraft> {
    match event {
        ThreadEvent::ThreadStarted { thread_id } => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::System,
            item_id: None,
            title: "thread started".to_owned(),
            body: thread_id.clone(),
            metadata: serde_json::json!({ "thread_id": thread_id }),
        }),
        ThreadEvent::TurnStarted => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::System,
            item_id: None,
            title: "turn started".to_owned(),
            body: String::new(),
            metadata: serde_json::json!({}),
        }),
        ThreadEvent::TurnCompleted { usage } => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::System,
            item_id: None,
            title: "turn completed".to_owned(),
            body: usage
                .as_ref()
                .map(|usage| {
                    format!(
                        "input={} cached_input={} output={}",
                        usage.input_tokens, usage.cached_input_tokens, usage.output_tokens
                    )
                })
                .unwrap_or_default(),
            metadata: match usage {
                Some(usage) => serde_json::json!({
                    "usage": {
                        "input_tokens": usage.input_tokens,
                        "cached_input_tokens": usage.cached_input_tokens,
                        "output_tokens": usage.output_tokens,
                    }
                }),
                None => serde_json::json!({}),
            },
        }),
        ThreadEvent::TurnFailed { error } => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::Error,
            item_id: None,
            title: "turn failed".to_owned(),
            body: error.message.clone(),
            metadata: serde_json::json!({ "message": &error.message }),
        }),
        ThreadEvent::Error { message } => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::Error,
            item_id: None,
            title: "stream error".to_owned(),
            body: message.clone(),
            metadata: serde_json::json!({ "message": message }),
        }),
        ThreadEvent::ItemStarted { item } => started_thread_item_piece(item),
        ThreadEvent::ItemUpdated { .. } => None,
        ThreadEvent::ItemCompleted { item } => completed_thread_item_piece(item),
    }
}

fn started_thread_item_piece(item: &ThreadItem) -> Option<OutputPieceDraft> {
    match item {
        ThreadItem::CommandExecution(command) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(command.id.clone()),
            title: "command started".to_owned(),
            body: command.command.clone(),
            metadata: serde_json::json!({
                "tool_type": "command",
                "status": "started",
                "command": &command.command,
            }),
        }),
        ThreadItem::McpToolCall(call) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(call.id.clone()),
            title: format!("mcp tool started: {}/{}", call.server, call.tool),
            body: format!("{}/{}", call.server, call.tool),
            metadata: serde_json::json!({
                "tool_type": "mcp",
                "status": "started",
                "server": &call.server,
                "tool": &call.tool,
                "arguments": &call.arguments,
            }),
        }),
        ThreadItem::DynamicToolCall(call) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(call.id.clone()),
            title: format!("dynamic tool started: {}", call.tool),
            body: call.tool.clone(),
            metadata: serde_json::json!({
                "tool_type": "dynamic",
                "status": "started",
                "tool": &call.tool,
                "arguments": &call.arguments,
            }),
        }),
        ThreadItem::CollabToolCall(call) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(call.id.clone()),
            title: format!("collaboration tool started: {}", call.tool),
            body: call.tool.clone(),
            metadata: serde_json::json!({
                "tool_type": "collaboration",
                "status": "started",
                "tool": &call.tool,
                "sender_thread_id": &call.sender_thread_id,
                "receiver_thread_id": &call.receiver_thread_id,
                "new_thread_id": &call.new_thread_id,
                "prompt": &call.prompt,
                "agent_status": &call.agent_status,
            }),
        }),
        _ => None,
    }
}

fn completed_thread_item_piece(item: &ThreadItem) -> Option<OutputPieceDraft> {
    match item {
        ThreadItem::AgentMessage(message) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ModelMessage,
            item_id: Some(message.id.clone()),
            title: if message.is_final_answer() {
                "final answer".to_owned()
            } else {
                "model output".to_owned()
            },
            body: message.text.clone(),
            metadata: serde_json::json!({
                "phase": message.phase.map(|phase| phase.as_str()),
                "final_answer": message.is_final_answer(),
            }),
        }),
        ThreadItem::Plan(plan) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ModelMessage,
            item_id: Some(plan.id.clone()),
            title: "plan".to_owned(),
            body: plan.text.clone(),
            metadata: serde_json::json!({ "item_type": "plan" }),
        }),
        ThreadItem::Reasoning(reasoning) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::Reasoning,
            item_id: Some(reasoning.id.clone()),
            title: "reasoning".to_owned(),
            body: reasoning.text.clone(),
            metadata: serde_json::json!({}),
        }),
        ThreadItem::CommandExecution(command) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(command.id.clone()),
            title: format!("command {:?}", command.status),
            body: command.command.clone(),
            metadata: serde_json::json!({
                "tool_type": "command",
                "status": format!("{:?}", command.status),
                "command": &command.command,
                "exit_code": command.exit_code,
                "output": &command.aggregated_output,
            }),
        }),
        ThreadItem::FileChange(change) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::FileChange,
            item_id: Some(change.id.clone()),
            title: format!("file change {:?}", change.status),
            body: format!("{} file(s)", change.changes.len()),
            metadata: serde_json::json!({
                "status": format!("{:?}", change.status),
                "changes": change.changes.iter().map(|change| {
                    serde_json::json!({
                        "path": &change.path,
                        "kind": format!("{:?}", change.kind),
                    })
                }).collect::<Vec<_>>(),
            }),
        }),
        ThreadItem::McpToolCall(call) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(call.id.clone()),
            title: format!("mcp tool {:?}: {}/{}", call.status, call.server, call.tool),
            body: format!("{}/{}", call.server, call.tool),
            metadata: serde_json::json!({
                "tool_type": "mcp",
                "status": format!("{:?}", call.status),
                "server": &call.server,
                "tool": &call.tool,
                "arguments": &call.arguments,
                "result": &call.result,
                "error": call.error.as_ref().map(|error| error.message.clone()),
            }),
        }),
        ThreadItem::DynamicToolCall(call) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(call.id.clone()),
            title: format!("dynamic tool {}: {}", call.status, call.tool),
            body: call.tool.clone(),
            metadata: serde_json::json!({
                "tool_type": "dynamic",
                "status": &call.status,
                "tool": &call.tool,
                "arguments": &call.arguments,
                "content_items": &call.content_items,
                "success": call.success,
                "duration_ms": call.duration_ms,
            }),
        }),
        ThreadItem::CollabToolCall(call) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(call.id.clone()),
            title: format!("collaboration tool {}: {}", call.status, call.tool),
            body: call.tool.clone(),
            metadata: serde_json::json!({
                "tool_type": "collaboration",
                "status": &call.status,
                "tool": &call.tool,
                "sender_thread_id": &call.sender_thread_id,
                "receiver_thread_id": &call.receiver_thread_id,
                "new_thread_id": &call.new_thread_id,
                "prompt": &call.prompt,
                "agent_status": &call.agent_status,
            }),
        }),
        ThreadItem::WebSearch(search) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(search.id.clone()),
            title: "web search".to_owned(),
            body: search.query.clone(),
            metadata: serde_json::json!({
                "tool_type": "web_search",
                "query": &search.query,
            }),
        }),
        ThreadItem::ImageView(image) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::ToolCall,
            item_id: Some(image.id.clone()),
            title: "image view".to_owned(),
            body: image.path.clone(),
            metadata: serde_json::json!({
                "tool_type": "image_view",
                "path": &image.path,
            }),
        }),
        ThreadItem::EnteredReviewMode(review) => {
            Some(review_mode_piece(review, "entered review mode"))
        }
        ThreadItem::ExitedReviewMode(review) => {
            Some(review_mode_piece(review, "exited review mode"))
        }
        ThreadItem::ContextCompaction(item) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::System,
            item_id: Some(item.id.clone()),
            title: "context compaction".to_owned(),
            body: String::new(),
            metadata: serde_json::json!({}),
        }),
        ThreadItem::TodoList(list) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::System,
            item_id: Some(list.id.clone()),
            title: "todo list".to_owned(),
            body: format!("{} item(s)", list.items.len()),
            metadata: serde_json::json!({
                "items": list.items.iter().map(|item| {
                    serde_json::json!({
                        "text": &item.text,
                        "completed": item.completed,
                    })
                }).collect::<Vec<_>>(),
            }),
        }),
        ThreadItem::Error(error) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::Error,
            item_id: Some(error.id.clone()),
            title: "error".to_owned(),
            body: error.message.clone(),
            metadata: serde_json::json!({ "message": &error.message }),
        }),
        ThreadItem::Unknown(unknown) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::System,
            item_id: unknown.id.clone(),
            title: format!(
                "unknown item {}",
                unknown.item_type.as_deref().unwrap_or("unknown")
            ),
            body: String::new(),
            metadata: serde_json::json!({
                "item_type": &unknown.item_type,
                "raw": &unknown.raw,
            }),
        }),
        ThreadItem::UserMessage(message) => Some(OutputPieceDraft {
            kind: AgentRunOutputKind::System,
            item_id: Some(message.id.clone()),
            title: "user message".to_owned(),
            body: String::new(),
            metadata: serde_json::json!({ "content_item_count": message.content.len() }),
        }),
    }
}

fn review_mode_piece(review: &ReviewModeItem, title: &'static str) -> OutputPieceDraft {
    OutputPieceDraft {
        kind: AgentRunOutputKind::System,
        item_id: Some(review.id.clone()),
        title: title.to_owned(),
        body: review.review.clone(),
        metadata: serde_json::json!({ "review": &review.review }),
    }
}

fn update_response_candidates(
    item: &ThreadItem,
    final_answer: &mut Option<String>,
    fallback_answer: &mut Option<String>,
) {
    let ThreadItem::AgentMessage(message) = item else {
        return;
    };
    if message.is_final_answer() {
        *final_answer = Some(message.text.clone());
    } else {
        *fallback_answer = Some(message.text.clone());
    }
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
        mode: AutomationMode::from_str(&run.mode)?,
        tool_name: AgentToolName::from_str(&run.tool_name)?,
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        items::{CreateWorkItem, claim_item, create_item, get_item},
        projects::{
            CreateProject, UpdateProjectSettings, create_project, get_project, update_settings,
        },
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
    fn prompt_includes_cli_context_without_agent_model_settings() {
        let item = WorkItemView {
            id: 42,
            project_id: 1,
            title: "Implement API relay".to_owned(),
            description: "Switch agent-facing CLI calls through HTTP.".to_owned(),
            state: Some("in_progress".to_owned()),
            labels: vec![
                patchbay_types::WorkItemLabelView {
                    id: 1,
                    project_id: 1,
                    work_item_id: 42,
                    key: "state".to_owned(),
                    value: Some("in_progress".to_owned()),
                    created_at: "2026-06-14T00:00:00Z".to_owned(),
                    updated_at: "2026-06-14T00:00:00Z".to_owned(),
                },
                patchbay_types::WorkItemLabelView {
                    id: 2,
                    project_id: 1,
                    work_item_id: 42,
                    key: CLAIMED_FROM_STATE_LABEL_KEY.to_owned(),
                    value: Some("ready".to_owned()),
                    created_at: "2026-06-14T00:00:00Z".to_owned(),
                    updated_at: "2026-06-14T00:00:00Z".to_owned(),
                },
            ],
            version: 3,
            claimed_by: Some("patchbay-run-1".to_owned()),
            claimed_at: None,
            claim_expires_at: None,
            finished_at: None,
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            created_at: "2026-06-14T00:00:00Z".to_owned(),
            updated_at: "2026-06-14T00:00:00Z".to_owned(),
            comment_count: 0,
        };
        let prompt = build_prompt(PromptContext {
            project_name: "demo",
            mode: AutomationMode::Execute,
            system_prompt: "",
            memory: "",
            memory_event_id: Some(7),
            item: Some(&item),
            agent_id: "patchbay-run-1",
            extra_prompt: None,
            create_pr: false,
        });

        assert!(prompt.contains("## Patchbay Agent Instructions"));
        assert!(
            prompt.contains("is the source of truth for work state, labels, and project memory")
        );
        assert!(prompt.contains("PATCHBAY_API_URL=<api-url>"));
        assert!(prompt.contains("PATCHBAY_CLAIMED_ITEM_ID=<item-id>"));
        assert!(prompt.contains("When `PATCHBAY_CLAIMED_ITEM_ID` is set"));
        assert!(
            prompt.contains(
                "`item list`, `item create`, and `item claim` do not use the claimed item"
            )
        );
        assert!(prompt.contains("patchbay item show [item-id] [--json]"));
        assert!(prompt.contains("patchbay item update [item-id]"));
        assert!(prompt.contains("--state <state-label>"));
        assert!(prompt.contains("patchbay label add [item-id]"));
        assert!(prompt.contains("State label: in_progress"));
        assert!(prompt.contains("Claimed from state label: ready"));
        assert!(prompt.contains("Release behavior: `patchbay item release` restores"));
        assert!(prompt.contains(AUTOMATION_BLOCKED_LABEL_KEY));
        assert!(prompt.contains("Labels: state=in_progress, patchbay:claimed-from-state=ready"));
        assert!(prompt.contains("--clear-agent-reasoning-effort"));
        assert!(prompt.contains("patchbay comment add [item-id]"));
        assert!(prompt.contains("patchbay automation runs [--limit N]"));
        assert!(prompt.contains("Project memory is tracked through Patchbay"));
        assert!(prompt.contains("not through Codex internal memory"));
        assert!(prompt.contains("full project memory snapshot"));
        assert!(prompt.contains("patchbay memory append --body"));
        assert!(prompt.contains("MemoryChanged event: #7"));
        assert!(!prompt.contains("PATCHBAY_DATABASE"));
        assert!(!prompt.contains("--project demo"));
        assert!(!prompt.contains("PATCHBAY_URL"));
        assert!(!prompt.contains("## Agent Model Settings"));
        assert!(!prompt.contains("Model: gpt-5-codex"));
        assert!(!prompt.contains("Reasoning effort: medium"));
        assert!(!prompt.contains("Use the Patchbay CLI for progress and final status"));
    }

    #[test]
    fn agent_environment_exposes_api_but_not_database() {
        let env = agent_environment(
            Path::new("/tmp/patchbay"),
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
            },
        )
        .await
        .unwrap();
        let project = get_project(&store, "demo").await.unwrap();
        let run = create_run(
            &store,
            project.id,
            AutomationMode::Execute,
            AgentToolName::Codex,
            None,
        )
        .await
        .unwrap();
        let agent_id = format!("patchbay-run-{}", run.id);
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
