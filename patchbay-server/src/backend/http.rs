use std::{convert::Infallible, io, path::PathBuf, process::Output, time::Duration};

use async_stream::stream;
use axum::{
    Extension, Form, Json, Router,
    extract::{
        Path, Query,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Redirect, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use crudkit_rs::impl_add_crud_routes;
use futures_core::Stream;
use leptos::prelude::LeptosOptions;
use leptos_axum::{LeptosRoutes, generate_route_list};
use rootcause::{Result, prelude::*};

use crate::{
    backend::{
        agent_tools, api,
        app_state::AppState,
        automation::{self, StartAutomation},
        automation_triggers::{self, CreateAutomationTrigger},
        codex_app_server,
        comments::{self, AddComment},
        crudkit_resources, events,
        items::{self, CreateWorkItem, UpdateWorkItem},
        projects::{self, UpdateProjectSettings},
        storage::Store,
        workspace::{self, WorkspaceOpenTarget},
    },
    frontend,
    shared::view_models::{
        AgentGitCommandPolicy, AgentGitHardResetPolicy, AgentReasoningEffort, AgentRunStatus,
        AgentToolName, AuthorType, AutomationActivation, AutomationEffect, DEFAULT_STATE_LABEL,
        ProcessSessionView, RevertStrategy, WorkspaceMode, WorktreeCleanupPolicy,
    },
};

impl_add_crud_routes!(
    crate::backend::crudkit_resources::CrudProjectResource,
    project
);
impl_add_crud_routes!(
    crate::backend::crudkit_resources::CrudWorkItemResource,
    work_item
);
impl_add_crud_routes!(
    crate::backend::crudkit_resources::CrudCommentResource,
    comment
);
impl_add_crud_routes!(
    crate::backend::crudkit_resources::CrudAgentToolResource,
    agent_tool
);
impl_add_crud_routes!(
    crate::backend::crudkit_resources::CrudAgentRunResource,
    agent_run
);
impl_add_crud_routes!(
    crate::backend::crudkit_resources::CrudAutomationTriggerResource,
    automation_trigger
);
impl_add_crud_routes!(
    crate::backend::crudkit_resources::CrudSwimLaneResource,
    swim_lane
);
impl_add_crud_routes!(
    crate::backend::crudkit_resources::CrudWorkItemStateResource,
    work_item_state
);

pub(crate) fn router(
    state: AppState,
    contexts: crudkit_resources::CrudContexts,
    leptos_options: LeptosOptions,
) -> Router<()> {
    let routes = generate_route_list(frontend::App);

    let mut crud_router = Router::new();
    crud_router = axum_project_crud_routes::add_crud_routes("/api", crud_router);
    crud_router = axum_work_item_crud_routes::add_crud_routes("/api", crud_router);
    crud_router = axum_comment_crud_routes::add_crud_routes("/api", crud_router);
    crud_router = axum_agent_tool_crud_routes::add_crud_routes("/api", crud_router);
    crud_router = axum_agent_run_crud_routes::add_crud_routes("/api", crud_router);
    crud_router = axum_automation_trigger_crud_routes::add_crud_routes("/api", crud_router);
    crud_router = axum_swim_lane_crud_routes::add_crud_routes("/api", crud_router);
    crud_router = axum_work_item_state_crud_routes::add_crud_routes("/api", crud_router);

    let leptos_shell = {
        let leptos_options = leptos_options.clone();
        move || frontend::shell(leptos_options.clone())
    };
    Router::<LeptosOptions>::new()
        .route("/system/pick-folder", post(pick_folder))
        .route("/system/database/open", post(open_database_directory))
        .route("/projects", post(create_project))
        .route("/projects/{project}/update", post(update_project))
        .route("/projects/{project}/delete", post(delete_project))
        .route(
            "/projects/{project}/system-prompt",
            post(update_system_prompt),
        )
        .route(
            "/projects/{project}/system-prompt/events/compact",
            post(compact_system_prompt_events),
        )
        .route("/projects/{project}/memory", post(update_memory))
        .route("/projects/{project}/memory/append", post(append_memory))
        .route(
            "/projects/{project}/memory/events/compact",
            post(compact_memory_events),
        )
        .route("/projects/{project}/settings", post(update_settings))
        .route(
            "/projects/{project}/settings/auto-commit",
            post(update_auto_commit),
        )
        .route(
            "/projects/{project}/settings/commit-policy",
            post(update_commit_policy),
        )
        .route(
            "/projects/{project}/workspace/open",
            post(open_project_workspace),
        )
        .route(
            "/projects/{project}/automation/start",
            post(start_automation),
        )
        .route("/projects/{project}/automation/stop", post(stop_automation))
        .route(
            "/projects/{project}/automation/recover-stale-claims",
            post(recover_stale_claims),
        )
        .route(
            "/projects/{project}/automation/cleanup-worktrees",
            post(cleanup_worktrees),
        )
        .route(
            "/projects/{project}/automation/runs/{run_id}/workspace/open",
            post(open_run_workspace),
        )
        .route(
            "/projects/{project}/automation/runs/{run_id}/cancel",
            post(cancel_run),
        )
        .route(
            "/projects/{project}/automation/triggers",
            post(create_automation_trigger),
        )
        .route(
            "/projects/{project}/automation/triggers/{trigger_id}/delete",
            post(delete_automation_trigger),
        )
        .route(
            "/projects/{project}/automation/triggers/{trigger_id}/update",
            post(update_automation_trigger),
        )
        .route(
            "/projects/{project}/automation/triggers/{trigger_id}/schedule-evaluation",
            post(schedule_automation_trigger_evaluation),
        )
        .route("/projects/{project}/items", post(create_item))
        .route(
            "/projects/{project}/items/{item_id}/update",
            post(update_item),
        )
        .route("/projects/{project}/items/{item_id}/move", post(move_item))
        .route(
            "/projects/{project}/items/{item_id}/delete",
            post(delete_item),
        )
        .route(
            "/projects/{project}/items/{item_id}/comments",
            post(add_comment),
        )
        .route(
            "/projects/{project}/items/{item_id}/labels",
            post(add_item_label),
        )
        .route(
            "/projects/{project}/items/{item_id}/labels/{label_id}/update",
            post(update_item_label),
        )
        .route(
            "/projects/{project}/items/{item_id}/labels/{label_id}/delete",
            post(delete_item_label),
        )
        .route("/agent-tools/discover", post(discover_agent_tools))
        .route("/codex/logout", post(logout_codex))
        .route("/api/projects/{project}", get(api::get_project))
        .route(
            "/api/projects/{project}/settings",
            get(api::get_project_settings),
        )
        .route(
            "/api/projects/{project}/memory",
            get(api::get_project_memory).put(api::set_project_memory),
        )
        .route(
            "/api/projects/{project}/memory/append",
            post(api::append_project_memory),
        )
        .route(
            "/api/projects/{project}/memory/events",
            get(api::list_project_memory_events),
        )
        .route(
            "/api/projects/{project}/memory/events/compact",
            post(api::compact_project_memory_events),
        )
        .route(
            "/api/projects/{project}/items",
            get(api::list_items).post(api::create_item),
        )
        .route(
            "/api/projects/{project}/labels",
            get(api::list_project_labels),
        )
        .route("/api/projects/{project}/items/claim", post(api::claim_item))
        .route(
            "/api/projects/{project}/items/{item_id}",
            get(api::get_item).patch(api::update_item),
        )
        .route(
            "/api/projects/{project}/items/{item_id}/labels",
            get(api::list_item_labels).post(api::add_item_label),
        )
        .route(
            "/api/projects/{project}/items/{item_id}/labels/{label_id}",
            axum::routing::patch(api::update_item_label).delete(api::delete_item_label),
        )
        .route(
            "/api/projects/{project}/items/{item_id}/progress",
            post(api::progress_item),
        )
        .route(
            "/api/projects/{project}/items/{item_id}/finish",
            post(api::finish_item),
        )
        .route(
            "/api/projects/{project}/items/{item_id}/release",
            post(api::release_item),
        )
        .route(
            "/api/projects/{project}/items/{item_id}/request-feedback",
            post(api::request_item_feedback),
        )
        .route(
            "/api/projects/{project}/items/{item_id}/comments",
            get(api::list_comments).post(api::add_comment),
        )
        .route(
            "/api/projects/{project}/automation/runs",
            get(api::list_runs),
        )
        .route(
            "/api/projects/{project}/automation/runs/{run_id}/log",
            get(api::get_run_log),
        )
        .route("/api/projects/{project}/events", get(project_events))
        .route(
            "/api/projects/{project}/automation/sessions",
            get(active_sessions),
        )
        .route(
            "/api/projects/{project}/items/{item_id}/events",
            get(item_events),
        )
        .route("/api/events/ws", get(ui_events_ws))
        .merge(crud_router.with_state(()))
        .leptos_routes(&leptos_options, routes, leptos_shell)
        .fallback(leptos_axum::file_and_error_handler(frontend::shell))
        .layer(Extension(state))
        .layer(Extension(contexts.project))
        .layer(Extension(contexts.work_item))
        .layer(Extension(contexts.comment))
        .layer(Extension(contexts.agent_tool))
        .layer(Extension(contexts.agent_run))
        .layer(Extension(contexts.automation_trigger))
        .layer(Extension(contexts.swim_lane))
        .layer(Extension(contexts.work_item_state))
        .with_state(leptos_options)
}

async fn error_response(err: impl Into<Report>) -> Response {
    let err = err.into();
    Redirect::to(&format!(
        "/error?message={}",
        urlencoding::encode(&err.to_string())
    ))
    .into_response()
}

#[derive(serde::Serialize)]
struct PickFolderResponse {
    path: Option<String>,
}

#[derive(serde::Serialize)]
struct ErrorJson {
    error: String,
}

async fn pick_folder() -> Response {
    match choose_folder_path().await {
        Ok(path) => Json(PickFolderResponse { path }).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorJson {
                error: err.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn choose_folder_path() -> Result<Option<String>> {
    match std::env::consts::OS {
        "macos" => choose_folder_path_macos().await,
        "linux" => choose_folder_path_linux().await,
        "windows" => choose_folder_path_windows().await,
        other => bail!("folder picker is not supported on {other}"),
    }
}

async fn choose_folder_path_macos() -> Result<Option<String>> {
    let output = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(r#"POSIX path of (choose folder with prompt "Choose project folder")"#)
        .output()
        .await
        .context("failed to start macOS folder picker")?;
    folder_path_from_output(output, &["User canceled"])
}

async fn choose_folder_path_linux() -> Result<Option<String>> {
    let pickers: [(&str, &[&str]); 2] = [
        ("zenity", &["--file-selection", "--directory"]),
        ("kdialog", &["--getexistingdirectory"]),
    ];
    for (command, args) in pickers {
        match tokio::process::Command::new(command)
            .args(args)
            .output()
            .await
        {
            Ok(output) => return folder_path_from_output(output, &[]),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err).context_with(|| format!("failed to start {command}"))?,
        }
    }
    bail!("no supported Linux folder picker found; install zenity or kdialog")
}

async fn choose_folder_path_windows() -> Result<Option<String>> {
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Choose project folder'
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    [Console]::WriteLine($dialog.SelectedPath)
}
"#;
    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .await
        .context("failed to start Windows folder picker")?;
    folder_path_from_output(output, &[])
}

fn folder_path_from_output(output: Output, cancel_markers: &[&str]) -> Result<Option<String>> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() {
        return Ok((!stdout.is_empty()).then_some(stdout));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let canceled_by_message = cancel_markers.iter().any(|marker| stderr.contains(marker));
    if canceled_by_message || output.status.code() == Some(1) && stdout.is_empty() {
        return Ok(None);
    }

    bail!(
        "folder picker failed{}",
        if stderr.trim().is_empty() {
            String::new()
        } else {
            format!(": {}", stderr.trim())
        }
    )
}

#[derive(serde::Deserialize)]
struct CreateProjectForm {
    name: String,
    display_name: Option<String>,
    path: String,
    default_agent_model: Option<String>,
    default_agent_reasoning_effort: Option<String>,
    system_prompt: Option<String>,
    memory: Option<String>,
}

async fn create_project(
    Extension(state): Extension<AppState>,
    Form(form): Form<CreateProjectForm>,
) -> Response {
    let display_name = form.display_name.filter(|value| !value.trim().is_empty());
    let path = PathBuf::from(form.path);
    let system_prompt = form.system_prompt.filter(|value| !value.trim().is_empty());
    let memory = form.memory.filter(|value| !value.trim().is_empty());
    let default_agent_reasoning_effort =
        match parse_optional_reasoning_effort(form.default_agent_reasoning_effort) {
            Ok(value) => value,
            Err(err) => return error_response(err).await,
        };
    match projects::create_project(
        &state.store,
        projects::CreateProject {
            name: form.name.clone(),
            display_name,
            path,
            default_agent_model: form.default_agent_model,
            default_agent_reasoning_effort,
            system_prompt,
            memory,
        },
    )
    .await
    {
        Ok(project) => Redirect::to(&format!("/?project={}", urlencoding::encode(&project.name)))
            .into_response(),
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct UpdateProjectForm {
    display_name: String,
    path: Option<String>,
}

async fn update_project(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<UpdateProjectForm>,
) -> Response {
    let display_name = Some(form.display_name);
    let path = form
        .path
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);
    match projects::update_project(
        &state.store,
        &project,
        projects::UpdateProject { display_name, path },
    )
    .await
    {
        Ok(_) => Redirect::to(&format!(
            "/projects?project={}",
            urlencoding::encode(&project)
        ))
        .into_response(),
        Err(err) => error_response(err).await,
    }
}

async fn delete_project(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    match projects::delete_project(&state.store, &project).await {
        Ok(()) => Redirect::to("/projects").into_response(),
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct ProjectTextForm {
    body: String,
}

async fn update_system_prompt(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<ProjectTextForm>,
) -> Response {
    match projects::update_system_prompt(&state.store, &project, form.body).await {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn compact_system_prompt_events(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    match projects::compact_system_prompt_events(&state.store, &project).await {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn update_memory(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<ProjectTextForm>,
) -> Response {
    match projects::update_memory_with_source(
        &state.store,
        &project,
        form.body,
        projects::ProjectChangeSource::User,
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn append_memory(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<ProjectTextForm>,
) -> Response {
    match projects::append_memory_with_source(
        &state.store,
        &project,
        form.body,
        projects::ProjectChangeSource::User,
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn compact_memory_events(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    match projects::compact_memory_events(&state.store, &project).await {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct UpdateSettingsForm {
    workspace_mode: String,
    max_code_edit_agents: i64,
    max_read_only_agents: Option<i64>,
    create_pr: Option<String>,
    auto_commit: Option<String>,
    commit_standard: Option<String>,
    revert_strategy: Option<String>,
    stale_claim_minutes: i64,
    worktree_cleanup_policy: String,
    default_agent_tool: String,
    default_agent_model: Option<String>,
    default_agent_reasoning_effort: Option<String>,
    agent_sandbox_mode: Option<String>,
    agent_extra_writable_roots: Option<String>,
}

async fn update_settings(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<UpdateSettingsForm>,
) -> Response {
    let workspace_mode = match form.workspace_mode.parse::<WorkspaceMode>() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let worktree_cleanup_policy = match form
        .worktree_cleanup_policy
        .parse::<WorktreeCleanupPolicy>()
    {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let revert_strategy = match form.revert_strategy {
        Some(value) => match value.parse::<RevertStrategy>() {
            Ok(value) => Some(value),
            Err(err) => return error_response(err).await,
        },
        None => None,
    };
    let default_agent_tool = match form.default_agent_tool.parse::<AgentToolName>() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let agent_sandbox_mode = match form.agent_sandbox_mode {
        Some(value) => match value.parse() {
            Ok(value) => Some(value),
            Err(err) => return error_response(err).await,
        },
        None => None,
    };
    let default_agent_reasoning_effort =
        match parse_optional_reasoning_effort(form.default_agent_reasoning_effort) {
            Ok(value) => value,
            Err(err) => return error_response(err).await,
        };
    let agent_extra_writable_roots = match form.agent_extra_writable_roots {
        Some(value) => match projects::parse_agent_extra_writable_roots_text(&value) {
            Ok(value) => Some(value),
            Err(err) => return error_response(err).await,
        },
        None => None,
    };

    match projects::update_settings(
        &state.store,
        &project,
        UpdateProjectSettings {
            workspace_mode: Some(workspace_mode),
            max_code_edit_agents: Some(form.max_code_edit_agents),
            max_read_only_agents: form.max_read_only_agents,
            create_pr: Some(form.create_pr.is_some()),
            auto_commit: parse_optional_checkbox(form.auto_commit),
            commit_standard: form.commit_standard,
            revert_strategy,
            stale_claim_minutes: Some(form.stale_claim_minutes),
            worktree_cleanup_policy: Some(worktree_cleanup_policy),
            default_agent_tool: Some(default_agent_tool),
            default_agent_model: Some(
                form.default_agent_model
                    .filter(|value| !value.trim().is_empty()),
            ),
            default_agent_reasoning_effort: Some(default_agent_reasoning_effort),
            agent_sandbox_mode,
            agent_extra_writable_roots,
            agent_git_command_policy: None,
        },
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct UpdateAutoCommitForm {
    enabled: bool,
}

async fn update_auto_commit(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    headers: HeaderMap,
    Form(form): Form<UpdateAutoCommitForm>,
) -> Response {
    match projects::update_settings(
        &state.store,
        &project,
        UpdateProjectSettings {
            auto_commit: Some(form.enabled),
            ..Default::default()
        },
    )
    .await
    {
        Ok(_) if is_background_form_request(&headers) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

fn is_background_form_request(headers: &HeaderMap) -> bool {
    headers
        .get("x-patchbay-background")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "true")
}

#[derive(serde::Deserialize)]
struct UpdateCommitPolicyForm {
    max_read_only_agents: Option<i64>,
    auto_commit: Option<String>,
    commit_standard: Option<String>,
    revert_strategy: String,
    git_add: Option<String>,
    git_commit: Option<String>,
    git_push: Option<String>,
    git_reset: Option<String>,
    git_hard_reset: String,
}

async fn update_commit_policy(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<UpdateCommitPolicyForm>,
) -> Response {
    let revert_strategy = match form.revert_strategy.parse::<RevertStrategy>() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let git_hard_reset = match form.git_hard_reset.parse::<AgentGitHardResetPolicy>() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let agent_git_command_policy = AgentGitCommandPolicy {
        add: form.git_add.is_some(),
        commit: form.git_commit.is_some(),
        push: form.git_push.is_some(),
        reset: form.git_reset.is_some(),
        hard_reset: git_hard_reset,
    };
    match projects::update_settings(
        &state.store,
        &project,
        UpdateProjectSettings {
            max_read_only_agents: form.max_read_only_agents,
            auto_commit: Some(form.auto_commit.is_some()),
            commit_standard: form.commit_standard,
            revert_strategy: Some(revert_strategy),
            agent_git_command_policy: Some(agent_git_command_policy),
            ..Default::default()
        },
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct OpenWorkspaceForm {
    target: String,
    return_to: Option<String>,
}

#[derive(serde::Deserialize)]
struct OpenDirectoryForm {
    return_to: Option<String>,
}

#[derive(serde::Deserialize)]
struct CancelRunForm {
    return_to: Option<String>,
}

async fn open_database_directory(
    Extension(state): Extension<AppState>,
    Form(form): Form<OpenDirectoryForm>,
) -> Response {
    let return_to = safe_return_to(form.return_to, "/".to_owned());
    let result = async {
        let directory = state
            .store
            .path()
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| report!("database path has no parent directory"))?;
        workspace::open_workspace_path(WorkspaceOpenTarget::Folder, directory).await
    }
    .await;

    match result {
        Ok(()) => Redirect::to(&return_to).into_response(),
        Err(err) => error_response(err).await,
    }
}

async fn open_project_workspace(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<OpenWorkspaceForm>,
) -> Response {
    let return_to = safe_return_to(
        form.return_to,
        format!("/?project={}", urlencoding::encode(&project)),
    );
    let target = match WorkspaceOpenTarget::parse(&form.target) {
        Ok(target) => target,
        Err(err) => return error_response(err).await,
    };
    let result = async {
        let path = workspace::project_workspace_path(&state.store, &project).await?;
        workspace::open_workspace_path(target, path).await
    }
    .await;

    match result {
        Ok(()) => Redirect::to(&return_to).into_response(),
        Err(err) => error_response(err).await,
    }
}

async fn open_run_workspace(
    Extension(state): Extension<AppState>,
    Path((project, run_id)): Path<(String, i64)>,
    Form(form): Form<OpenWorkspaceForm>,
) -> Response {
    let return_to = safe_return_to(
        form.return_to,
        format!(
            "/projects/{}/automation/runs/{}/log",
            urlencoding::encode(&project),
            run_id
        ),
    );
    let target = match WorkspaceOpenTarget::parse(&form.target) {
        Ok(target) => target,
        Err(err) => return error_response(err).await,
    };
    let result = async {
        let run = automation::get_run(&state.store, &project, run_id).await?;
        workspace::open_workspace_path(target, run.working_dir).await
    }
    .await;

    match result {
        Ok(()) => Redirect::to(&return_to).into_response(),
        Err(err) => error_response(err).await,
    }
}

async fn cancel_run(
    Extension(state): Extension<AppState>,
    Path((project, run_id)): Path<(String, i64)>,
    Form(form): Form<CancelRunForm>,
) -> Response {
    let return_to = safe_return_to(
        form.return_to,
        format!(
            "/projects/{}/automation/runs/{}/log",
            urlencoding::encode(&project),
            run_id
        ),
    );
    let result: Result<()> = async {
        let run = automation::get_run(&state.store, &project, run_id).await?;
        if run.status != AgentRunStatus::Running {
            bail!("automation run {run_id} is not running");
        }
        if !state.sessions.cancel_run(&project, run_id).await {
            bail!("automation run {run_id} does not have an active session");
        }
        Ok(())
    }
    .await;

    match result {
        Ok(()) => Redirect::to(&return_to).into_response(),
        Err(err) => error_response(err).await,
    }
}

fn safe_return_to(return_to: Option<String>, fallback: String) -> String {
    return_to
        .filter(|target| target.starts_with('/') && !target.starts_with("//"))
        .unwrap_or(fallback)
}

#[derive(serde::Deserialize)]
struct StartAutomationForm {
    tool: Option<String>,
    item_id: Option<i64>,
    prompt: Option<String>,
    mutability: Option<String>,
}

async fn start_automation(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<StartAutomationForm>,
) -> Response {
    let tool = match form.tool.filter(|value| !value.trim().is_empty()) {
        Some(tool) => match tool.parse::<AgentToolName>() {
            Ok(value) => Some(value),
            Err(err) => return error_response(err).await,
        },
        None => None,
    };
    let is_one_shot = tool.is_some()
        || form.item_id.is_some()
        || form
            .mutability
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        || form
            .prompt
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
    let mutability = match form.mutability.filter(|value| !value.trim().is_empty()) {
        Some(value) => match value.parse() {
            Ok(value) => Some(value),
            Err(err) => return error_response(err).await,
        },
        None => None,
    };

    let result = if is_one_shot {
        automation::start_one_automation_run_in_background(
            state.store.clone(),
            project.clone(),
            StartAutomation {
                tool,
                work_item_id: form.item_id,
                work_item_selector: None,
                extra_prompt: form.prompt.filter(|value| !value.trim().is_empty()),
                mutability,
                trigger: None,
            },
            Some(state.sessions.clone()),
        )
        .await
        .map(|_| ())
    } else {
        let status = codex_app_server::app_server_status(&state.store).await;
        let usable = status.usable;
        let message = status.message.clone();
        *state.codex_status.write().await = status;
        events::publish_codex_status_changed();
        if !usable {
            return error_response(report!(message)).await;
        }
        state
            .automation_controller
            .start_project(state.store.clone(), project.clone(), state.sessions.clone())
            .await
    };

    match result {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn stop_automation(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    match state
        .automation_controller
        .stop_project(&project, &state.sessions)
        .await
    {
        Ok(()) => match automation::stop_automation(&state.store, &project).await {
            Ok(_) => Redirect::to(&format!("/?project={}", urlencoding::encode(&project)))
                .into_response(),
            Err(err) => error_response(err).await,
        },
        Err(err) => error_response(err).await,
    }
}

async fn recover_stale_claims(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    match automation::recover_stale_claims_for_project(&state.store, &project, None).await {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn cleanup_worktrees(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    match automation::cleanup_worktrees(&state.store, &project, None).await {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn active_sessions(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Json<Vec<ProcessSessionView>> {
    Json(state.sessions.list_for_project(&project).await)
}

#[derive(serde::Deserialize)]
struct CreateAutomationTriggerForm {
    name: String,
    #[serde(default = "default_automation_activation", alias = "kind")]
    activation: String,
    #[serde(default = "default_automation_effect")]
    effect: String,
    #[serde(default = "default_automation_schedule")]
    schedule: String,
    tool: Option<String>,
    #[serde(default = "default_automation_mutability")]
    mutability: String,
    work_item_selector: Option<String>,
    priority: Option<i64>,
    prompt: Option<String>,
}

async fn create_automation_trigger(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<CreateAutomationTriggerForm>,
) -> Response {
    let activation = match form.activation.parse::<AutomationActivation>() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let effect = match form.effect.parse::<AutomationEffect>() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let tool_name = match form.tool.filter(|value| !value.trim().is_empty()) {
        Some(tool) => match tool.parse::<AgentToolName>() {
            Ok(value) => Some(value),
            Err(err) => return error_response(err).await,
        },
        None => None,
    };
    let work_item_selector =
        match automation_triggers::selector_from_storage(form.work_item_selector.as_deref()) {
            Ok(selector) => selector,
            Err(err) => return error_response(err).await,
        };
    let mutability = match form.mutability.parse() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    match automation_triggers::create_trigger(
        &state.store,
        &project,
        CreateAutomationTrigger {
            name: form.name,
            enabled: true,
            activation,
            effect,
            schedule: form.schedule,
            tool_name,
            mutability,
            prompt: form.prompt.unwrap_or_default(),
            work_item_selector,
            priority: form.priority.unwrap_or_default(),
        },
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn delete_automation_trigger(
    Extension(state): Extension<AppState>,
    Path((project, trigger_id)): Path<(String, i64)>,
) -> Response {
    match automation_triggers::delete_trigger(&state.store, &project, trigger_id).await {
        Ok(()) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct UpdateAutomationTriggerForm {
    name: String,
    #[serde(default = "default_automation_activation", alias = "kind")]
    activation: String,
    #[serde(default = "default_automation_effect")]
    effect: String,
    #[serde(default = "default_automation_schedule")]
    schedule: String,
    #[serde(default = "default_automation_mutability")]
    mutability: String,
    enabled: Option<String>,
    work_item_selector: Option<String>,
    priority: Option<i64>,
    prompt: Option<String>,
}

async fn update_automation_trigger(
    Extension(state): Extension<AppState>,
    Path((project, trigger_id)): Path<(String, i64)>,
    Form(form): Form<UpdateAutomationTriggerForm>,
) -> Response {
    let activation = match form.activation.parse::<AutomationActivation>() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let effect = match form.effect.parse::<AutomationEffect>() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    let work_item_selector =
        match automation_triggers::selector_from_storage(form.work_item_selector.as_deref()) {
            Ok(selector) => selector,
            Err(err) => return error_response(err).await,
        };
    let mutability = match form.mutability.parse() {
        Ok(value) => value,
        Err(err) => return error_response(err).await,
    };
    match automation_triggers::update_trigger(
        &state.store,
        &project,
        trigger_id,
        automation_triggers::UpdateAutomationTrigger {
            name: form.name,
            enabled: form.enabled.is_some(),
            activation,
            effect,
            schedule: form.schedule,
            mutability,
            prompt: form.prompt.unwrap_or_default(),
            work_item_selector,
            priority: form.priority,
        },
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn schedule_automation_trigger_evaluation(
    Extension(state): Extension<AppState>,
    Path((project, trigger_id)): Path<(String, i64)>,
) -> Response {
    match automation_triggers::schedule_trigger_evaluation(&state.store, &project, trigger_id).await
    {
        Ok(_) => Redirect::to(&format!(
            "/automation?project={}",
            urlencoding::encode(&project)
        ))
        .into_response(),
        Err(err) => error_response(err).await,
    }
}

fn default_automation_activation() -> String {
    AutomationActivation::WorkItem.as_storage().to_owned()
}

fn default_automation_effect() -> String {
    AutomationEffect::ConsumeWork.as_storage().to_owned()
}

fn default_automation_schedule() -> String {
    "@every 15s".to_owned()
}

fn default_automation_mutability() -> String {
    patchbay_types::AutomationRunMutability::Mutating
        .as_storage()
        .to_owned()
}

#[derive(serde::Deserialize)]
struct DiscoverAgentToolsForm {
    project: Option<String>,
    return_to: Option<String>,
}

async fn discover_agent_tools(
    Extension(state): Extension<AppState>,
    Form(form): Form<DiscoverAgentToolsForm>,
) -> Response {
    match agent_tools::discover_tools(&state.store).await {
        Ok(_) => {
            let status = codex_app_server::app_server_status(&state.store).await;
            *state.codex_status.write().await = status;
            events::publish_agent_tool_changed();
            events::publish_codex_status_changed();
            let target = codex_return_target(form.return_to, form.project);
            Redirect::to(&target).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

async fn logout_codex(
    Extension(state): Extension<AppState>,
    Form(form): Form<DiscoverAgentToolsForm>,
) -> Response {
    match codex_app_server::logout_current_account(&state.store).await {
        Ok(status) => {
            *state.codex_status.write().await = status;
            events::publish_codex_status_changed();
            let target = codex_return_target(form.return_to, form.project);
            Redirect::to(&target).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

fn codex_return_target(return_to: Option<String>, project: Option<String>) -> String {
    return_to
        .filter(|target| target.starts_with('/') && !target.starts_with("//"))
        .or_else(|| {
            project
                .filter(|project| !project.trim().is_empty())
                .map(|project| format!("/?project={}", urlencoding::encode(&project)))
        })
        .unwrap_or_else(|| "/projects".to_owned())
}

#[derive(serde::Deserialize)]
struct CreateItemForm {
    title: String,
    description: String,
    state: Option<String>,
    agent_model_override: Option<String>,
    agent_reasoning_effort_override: Option<String>,
}

async fn create_item(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Form(form): Form<CreateItemForm>,
) -> Response {
    let item_state = parse_optional_state_label(form.state);
    match items::create_item(
        &state.store,
        &project,
        CreateWorkItem {
            title: form.title,
            description: form.description,
            state: item_state,
            agent_model_override: form
                .agent_model_override
                .filter(|value| !value.trim().is_empty()),
            agent_reasoning_effort_override: match parse_optional_reasoning_effort(
                form.agent_reasoning_effort_override,
            ) {
                Ok(value) => value,
                Err(err) => return error_response(err).await,
            },
        },
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

fn parse_optional_reasoning_effort(value: Option<String>) -> Result<Option<AgentReasoningEffort>> {
    match value.filter(|value| !value.trim().is_empty()) {
        Some(value) => Ok(Some(value.parse::<AgentReasoningEffort>()?)),
        None => Ok(None),
    }
}

fn parse_optional_checkbox(value: Option<String>) -> Option<bool> {
    value.map(|value| {
        !matches!(
            value.trim().to_lowercase().as_str(),
            "" | "0" | "false" | "off" | "no"
        )
    })
}

#[derive(serde::Deserialize)]
struct UpdateItemForm {
    title: String,
    description: String,
    version: i64,
    agent_model_override: Option<String>,
    agent_reasoning_effort_override: Option<String>,
}

async fn update_item(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Form(form): Form<UpdateItemForm>,
) -> Response {
    let agent_reasoning_effort_override =
        match parse_optional_reasoning_effort(form.agent_reasoning_effort_override) {
            Ok(value) => value,
            Err(err) => return error_response(err).await,
        };
    match items::update_item(
        &state.store,
        &project,
        item_id,
        UpdateWorkItem {
            title: Some(form.title),
            description: Some(form.description),
            state: None,
            agent_model_override: Some(
                form.agent_model_override
                    .filter(|value| !value.trim().is_empty()),
            ),
            agent_reasoning_effort_override: Some(agent_reasoning_effort_override),
            expect_version: Some(form.version),
        },
    )
    .await
    {
        Ok(_) => Redirect::to(&format!(
            "/projects/{}/items/{}",
            urlencoding::encode(&project),
            item_id
        ))
        .into_response(),
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct MoveItemForm {
    state: String,
    version: i64,
}

async fn move_item(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Form(form): Form<MoveItemForm>,
) -> Response {
    let parsed_state = parse_state_label(form.state);

    match items::move_item(
        &state.store,
        &project,
        item_id,
        parsed_state,
        Some(form.version),
    )
    .await
    {
        Ok(_) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

fn parse_optional_state_label(value: Option<String>) -> String {
    value
        .and_then(|value| {
            let value = value.trim().to_owned();
            (!value.is_empty()).then_some(value)
        })
        .unwrap_or_else(|| DEFAULT_STATE_LABEL.to_owned())
}

fn parse_state_label(value: String) -> String {
    let value = value.trim().to_owned();
    if value.is_empty() {
        DEFAULT_STATE_LABEL.to_owned()
    } else {
        value
    }
}

async fn delete_item(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
) -> Response {
    match items::delete_item(&state.store, &project, item_id).await {
        Ok(()) => {
            Redirect::to(&format!("/?project={}", urlencoding::encode(&project))).into_response()
        }
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct AddItemLabelForm {
    key: String,
    value: Option<String>,
    version: i64,
}

async fn add_item_label(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Form(form): Form<AddItemLabelForm>,
) -> Response {
    match items::add_label(
        &state.store,
        &project,
        item_id,
        form.key,
        form.value,
        Some(form.version),
    )
    .await
    {
        Ok(_) => item_redirect(&project, item_id),
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct UpdateItemLabelForm {
    key: String,
    value: Option<String>,
    version: i64,
}

async fn update_item_label(
    Extension(state): Extension<AppState>,
    Path((project, item_id, label_id)): Path<(String, i64, i64)>,
    Form(form): Form<UpdateItemLabelForm>,
) -> Response {
    match items::update_label(
        &state.store,
        &project,
        item_id,
        label_id,
        Some(form.key),
        Some(form.value),
        Some(form.version),
    )
    .await
    {
        Ok(_) => item_redirect(&project, item_id),
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct DeleteItemLabelForm {
    version: i64,
}

async fn delete_item_label(
    Extension(state): Extension<AppState>,
    Path((project, item_id, label_id)): Path<(String, i64, i64)>,
    Form(form): Form<DeleteItemLabelForm>,
) -> Response {
    match items::delete_label(
        &state.store,
        &project,
        item_id,
        label_id,
        Some(form.version),
    )
    .await
    {
        Ok(_) => item_redirect(&project, item_id),
        Err(err) => error_response(err).await,
    }
}

fn item_redirect(project: &str, item_id: i64) -> Response {
    Redirect::to(&format!(
        "/projects/{}/items/{}",
        urlencoding::encode(project),
        item_id
    ))
    .into_response()
}

#[derive(serde::Deserialize)]
struct AddCommentForm {
    body: String,
    author_name: Option<String>,
}

async fn add_comment(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Form(form): Form<AddCommentForm>,
) -> Response {
    let author_name = form.author_name.filter(|value| !value.trim().is_empty());
    match comments::add_comment(
        &state.store,
        &project,
        item_id,
        AddComment {
            author_type: AuthorType::User,
            author_name,
            body: form.body,
        },
    )
    .await
    {
        Ok(_) => Redirect::to(&format!(
            "/projects/{}/items/{}",
            urlencoding::encode(&project),
            item_id
        ))
        .into_response(),
        Err(err) => error_response(err).await,
    }
}

#[derive(serde::Deserialize)]
struct EventsQuery {
    since: Option<i64>,
}

async fn project_events(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    event_stream(state.store.clone(), project, None, query.since)
}

async fn item_events(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Query(query): Query<EventsQuery>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    event_stream(state.store.clone(), project, Some(item_id), query.since)
}

async fn ui_events_ws(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_ui_events_socket).into_response()
}

async fn handle_ui_events_socket(mut socket: WebSocket) {
    let mut receiver = events::subscribe();
    loop {
        match receiver.recv().await {
            Ok(event) => match serde_json::to_string(&event) {
                Ok(body) => {
                    if socket.send(Message::Text(body.into())).await.is_err() {
                        break;
                    }
                }
                Err(err) => {
                    tracing::warn!("failed to serialize UI event: {err}");
                }
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

fn event_stream(
    store: Store,
    project: String,
    item_id: Option<i64>,
    since: Option<i64>,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let events = stream! {
        let mut last_id = since;
        loop {
            match items::list_events(&store, &project, item_id, last_id).await {
                Ok(new_events) => {
                    for event in new_events {
                        last_id = Some(event.id);
                        let response = Event::default()
                            .id(event.id.to_string())
                            .event(event.event_type.clone())
                            .json_data(&event)
                            .unwrap_or_else(|err| {
                                Event::default()
                                    .event("error")
                                    .data(format!("failed to serialize event: {err}"))
                            });
                        yield Ok(response);
                    }
                }
                Err(err) => {
                    yield Ok(Event::default().event("error").data(err.to_string()));
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    Sse::new(events).keep_alive(KeepAlive::default())
}
