use std::collections::HashMap;

#[cfg(feature = "ssr")]
use crate::backend::{app_state, page_data};
use crate::{
    frontend::{
        crudkit::{
            AutomationTableKind, SwimLanesPanel, WorkItemStatesPanel, WorkItemsPanel,
            agent_tools_panel, automation_triggers_crudkit_instance, crudkit_i64_id,
            projects_panel, selected_trigger_id_from_context, work_items_crudkit_config_for_view,
        },
        live_events::{LiveEventsProvider, event_scopes_named_project, refetch_on_live_event},
        rich_text::rich_text_plain_text,
        routes::routes,
        work_item_creation::{
            CreateItemOpenRequest, CreateItemStateOption, default_state_identifier,
            state_identifier_from_lane_filter, state_options_for_open_request,
            state_options_from_project_states,
        },
    },
    shared::view_models::{
        AUTOMATION_BLOCKED_LABEL_KEY, AgentCommitOutcome, AgentGitHardResetPolicy,
        AgentRunOutputKind, AgentRunOutputPiece, AgentRunStatus, AgentRunTokenUsageView,
        AgentRunView, AuthorType, AutomationStatusView, CLAIMED_FROM_STATE_LABEL_KEY,
        CodexAppServerStatusView, CodexAuthSetupView, CodexRateLimitView, CodexUsageSummaryView,
        CommentView, DEFAULT_STATE_LABEL, FEEDBACK_REQUESTED_LABEL_KEY, ProjectGitStatusView,
        ProjectLabelView, ProjectMemoryEventRefView, ProjectMemoryEventView, ProjectSettingsView,
        ProjectSystemPromptEventView, ProjectView, RevertStrategy, RunLogView, STATE_LABEL_KEY,
        SwimLaneView, UiEvent, WorkItemClaimSourceView, WorkItemLabelView,
        WorkItemRelationshipDirection, WorkItemRelationshipItemSummary,
        WorkItemRelationshipListEntry, WorkItemStateView, WorkItemView, WorkspaceEditorView,
        WorkspaceMode,
    },
};
use crudkit_leptos::crud_instance::CrudInstanceContext;
use crudkit_leptos::crud_instance_mgr::CrudInstanceMgr;
use crudkit_leptos::crudkit_core::condition::{
    Condition, ConditionClause, ConditionClauseValue, ConditionElement, Operator,
};
use crudkit_leptos::{
    crud_instance_config::{CrudCreateActionsPlacement, CrudNavigationConfig},
    crudkit_web::view::SerializableCrudView,
    prelude::*,
};
use leptonic::components::prelude::{
    Icon, LeptonicTheme, Modal, ModalBody, ModalFooter, ModalHeader, ModalTitle, Root, Select,
};
#[cfg(not(feature = "ssr"))]
use leptonic::components::prelude::{Toast, ToastTimeout, ToastVariant, Toasts};
use leptonic::prelude::icondata;
use leptos::prelude::LeptosOptions;
use leptos::prelude::*;
#[cfg(feature = "ssr")]
use leptos_axum::{ResponseOptions, ResponseParts};
use leptos_meta::{Meta, MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::hooks::{use_navigate, use_params_map};
use leptos_router::{
    NavigateOptions,
    components::{Outlet, Router},
    hooks::use_query_map,
};
use leptos_use::use_interval_fn;
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
#[cfg(not(feature = "ssr"))]
use uuid::Uuid;

const TOOL_OUTPUT_PREVIEW_CHARS: usize = 1200;
const BOARD_ITEMS_REFRESH_INTERVAL_MS: u64 = 30_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BoardPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub selected_project: Option<String>,
    pub selected_project_view: Option<ProjectView>,
    pub settings: Option<ProjectSettingsView>,
    pub workspace_editors: Vec<WorkspaceEditorView>,
    pub system_prompt_events: Vec<ProjectSystemPromptEventView>,
    pub memory_events: Vec<ProjectMemoryEventView>,
    pub automation_status: Option<AutomationStatusView>,
    pub automation_running: bool,
    pub items: Vec<WorkItemView>,
    pub swim_lanes: Vec<SwimLaneView>,
    pub work_item_states: Vec<WorkItemStateView>,
    pub label_suggestions: Vec<ProjectLabelView>,
    pub misconfigured_item_count: i64,
    pub api_base_url: String,
    pub codex_status: CodexAppServerStatusView,
    pub runtime: RuntimeConfigView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BoardItemsSection {
    pub items: Vec<WorkItemView>,
    pub swim_lanes: Vec<SwimLaneView>,
    pub work_item_states: Vec<WorkItemStateView>,
    pub misconfigured_item_count: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunsPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub selected_project: Option<String>,
    pub automation_status: Option<AutomationStatusView>,
    pub automation_running: bool,
    pub run_sessions: Vec<BoardRunSessionView>,
    pub workspace_editors: Vec<WorkspaceEditorView>,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunsSection {
    pub automation_status: AutomationStatusView,
    pub automation_running: bool,
    pub run_sessions: Vec<BoardRunSessionView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeConfigView {
    pub database_path: String,
    pub database_directory: String,
    pub codex_home_path: String,
    pub codex_config_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BoardRunSessionView {
    pub run: AgentRunView,
    pub prompt: Option<String>,
    pub output: Vec<AgentRunOutputPiece>,
    pub active: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectsPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub selected_project: Option<String>,
    pub api_base_url: String,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ItemPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub project: String,
    pub item: WorkItemView,
    pub comments: Vec<CommentView>,
    pub relationships: Vec<WorkItemRelationshipListEntry>,
    pub label_suggestions: Vec<ProjectLabelView>,
    pub work_item_states: Vec<WorkItemStateView>,
    pub automation_runs: Vec<AgentRunView>,
    pub api_base_url: String,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunLogPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub project: String,
    pub run_log: RunLogView,
    pub workspace_editors: Vec<WorkspaceEditorView>,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TriggersPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub selected_project: Option<String>,
    pub selected_project_view: Option<ProjectView>,
    pub workspace_editors: Vec<WorkspaceEditorView>,
    pub api_base_url: String,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CodexStatusPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub selected_project: Option<String>,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ApiDocsPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub selected_project: Option<String>,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone)]
enum CachedRoutePage {
    Board(Box<BoardPage>),
    Projects(Box<ProjectsPage>),
    Triggers(Box<TriggersPage>),
    Runs(Box<RunsPage>),
    Codex(Box<CodexStatusPage>),
    Item(Box<ItemPage>),
    RunLog(Box<RunLogPage>),
    ApiDocs(Box<ApiDocsPage>),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ActivePage {
    Board,
    Triggers,
    Runs,
    Codex,
    Projects,
    Api,
}

#[derive(Clone)]
struct TopBarAutomation {
    project: String,
    running: bool,
    workspace_mode: WorkspaceMode,
    auto_commit: ReadSignal<bool>,
    set_auto_commit: WriteSignal<bool>,
}

#[derive(Clone, Copy)]
struct RoutePageCacheContext {
    pages: ReadSignal<HashMap<String, CachedRoutePage>>,
    set_pages: WriteSignal<HashMap<String, CachedRoutePage>>,
}

fn provide_route_page_cache_context() {
    let (pages, set_pages) = signal(HashMap::new());
    provide_context(RoutePageCacheContext { pages, set_pages });
}

#[derive(Clone, Copy)]
struct WorkItemStatesContext {
    states: ReadSignal<Vec<WorkItemStateView>>,
    set_states: WriteSignal<Vec<WorkItemStateView>>,
}

fn provide_work_item_states_context(
    initial_states: Vec<WorkItemStateView>,
) -> WorkItemStatesContext {
    let (states, set_states) = signal(initial_states);
    let context = WorkItemStatesContext { states, set_states };
    provide_context(context);
    context
}

#[derive(Clone, Debug, PartialEq)]
struct ProjectSelectOption {
    name: String,
    display_name: String,
    active: bool,
}

#[allow(non_snake_case)]
pub fn shell(options: LeptosOptions) -> impl IntoView {
    provide_meta_context();

    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <Meta name="description" content="Patchbay project work orchestration"/>
                <Title text="Patchbay"/>
                <HydrationScripts options=options.clone()/>
                <Stylesheet id="leptos" href=options.css_path()/>
                <MetaTags/>
                <AutoReload options=options.clone()/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Root default_theme=LeptonicTheme::Light>
            <Router>
                {routes::generated_routes()}
            </Router>
        </Root>
    }
}

#[component]
pub fn MainLayout() -> impl IntoView {
    provide_route_page_cache_context();

    view! {
        <CrudInstanceMgr>
            <LiveEventsProvider/>
            <Outlet/>
        </CrudInstanceMgr>
    }
}

#[component]
pub fn PageBoard() -> impl IntoView {
    let selected_project = selected_project_signal();
    let selected_project_for_cache = selected_project;
    let api_base_url = api_base_url();
    let page = LocalResource::new(move || {
        let selected_project = selected_project.get();
        let cache_key = selected_project_page_cache_key("board", selected_project.as_deref());
        let api_base_url = api_base_url.clone();
        async move {
            (
                cache_key,
                load_board_page(selected_project, api_base_url).await,
            )
        }
    });

    view! {
        <Title text="Patchbay"/>
        {cached_page_view(
            move || selected_project_page_cache_key(
                "board",
                selected_project_for_cache.get().as_deref(),
            ),
            page,
            board_content,
            cache_board_page,
            board_page_from_cache,
        )}
    }
}

#[server(prefix = "/leptos")]
async fn load_board_page(
    selected_project: Option<String>,
    api_base_url: String,
) -> Result<BoardPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    page_data::board_page_data(
        &state.store,
        &state.automation_controller,
        codex_status,
        selected_project.as_deref(),
        api_base_url,
    )
    .await
    .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server(prefix = "/leptos")]
async fn load_board_items_section(project: String) -> Result<BoardItemsSection, ServerFnError> {
    let state = app_state::app_state();
    page_data::board_items_section(&state.store, &project)
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[component]
pub fn PageProjects() -> impl IntoView {
    let selected_project = selected_project_signal();
    let selected_project_for_cache = selected_project;
    let api_base_url = api_base_url();
    let api_base_url_for_panel = api_base_url.clone();
    let page = LocalResource::new(move || {
        let selected_project = selected_project.get();
        let cache_key = selected_project_page_cache_key("projects", selected_project.as_deref());
        let api_base_url = api_base_url.clone();
        async move {
            (
                cache_key,
                load_projects_page(selected_project, api_base_url).await,
            )
        }
    });

    view! {
        <Title text="Projects"/>
        {cached_page_view(
            move || selected_project_page_cache_key(
                "projects",
                selected_project_for_cache.get().as_deref(),
            ),
            page,
            projects_content,
            cache_projects_page,
            projects_page_from_cache,
        )}
        <div class="page-shell projects-page crudkit-tools-shell">
            {agent_tools_panel(api_base_url_for_panel)}
        </div>
    }
}

#[server(prefix = "/leptos")]
async fn load_projects_page(
    selected_project: Option<String>,
    api_base_url: String,
) -> Result<ProjectsPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    page_data::projects_page_data(
        &state.store,
        &state.automation_controller,
        codex_status,
        selected_project.as_deref(),
        api_base_url,
    )
    .await
    .map_err(|err| ServerFnError::new(err.to_string()))
}

#[component]
pub fn PageTriggers() -> impl IntoView {
    let selected_project = selected_project_signal();
    let selected_project_for_cache = selected_project;
    let api_base_url = api_base_url();
    let page = LocalResource::new(move || {
        let selected_project = selected_project.get();
        let cache_key = selected_project_page_cache_key("triggers", selected_project.as_deref());
        let api_base_url = api_base_url.clone();
        async move {
            (
                cache_key,
                load_triggers_page(selected_project, api_base_url).await,
            )
        }
    });

    view! {
        <Title text="Automation"/>
        {cached_page_view(
            move || selected_project_page_cache_key(
                "triggers",
                selected_project_for_cache.get().as_deref(),
            ),
            page,
            triggers_content,
            cache_triggers_page,
            triggers_page_from_cache,
        )}
    }
}

#[server(prefix = "/leptos")]
async fn load_triggers_page(
    selected_project: Option<String>,
    api_base_url: String,
) -> Result<TriggersPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    page_data::triggers_page_data(
        &state.store,
        &state.automation_controller,
        codex_status,
        selected_project.as_deref(),
        api_base_url,
    )
    .await
    .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server(prefix = "/leptos")]
async fn load_trigger_run_sessions(
    project: String,
    trigger_id: i64,
) -> Result<Vec<BoardRunSessionView>, ServerFnError> {
    let state = app_state::app_state();
    page_data::trigger_run_sessions(&state.store, &state.sessions, &project, trigger_id)
        .await
        .map_err(|err| ServerFnError::new(err.to_string()))
}

#[component]
pub fn PageRuns() -> impl IntoView {
    let selected_project = selected_project_signal();
    let selected_project_for_cache = selected_project;
    let page = LocalResource::new(move || {
        let selected_project = selected_project.get();
        let cache_key = selected_project_page_cache_key("runs", selected_project.as_deref());
        async move { (cache_key, load_runs_page(selected_project).await) }
    });
    refetch_on_live_event(page, runs_page_event_matches);

    view! {
        <Title text="Runs"/>
        {cached_page_view(
            move || selected_project_page_cache_key(
                "runs",
                selected_project_for_cache.get().as_deref(),
            ),
            page,
            runs_content,
            cache_runs_page,
            runs_page_from_cache,
        )}
    }
}

#[server(prefix = "/leptos")]
async fn load_runs_page(selected_project: Option<String>) -> Result<RunsPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    page_data::runs_page_data(
        &state.store,
        &state.sessions,
        &state.automation_controller,
        codex_status,
        selected_project.as_deref(),
    )
    .await
    .map_err(|err| ServerFnError::new(err.to_string()))
}

#[server(prefix = "/leptos")]
async fn load_runs_section(project: String) -> Result<RunsSection, ServerFnError> {
    let state = app_state::app_state();
    page_data::runs_section(
        &state.store,
        &state.sessions,
        &state.automation_controller,
        &project,
    )
    .await
    .map_err(|err| ServerFnError::new(err.to_string()))
}

#[component]
pub fn PageCodex() -> impl IntoView {
    let selected_project = selected_project_signal();
    let selected_project_for_cache = selected_project;
    let page = LocalResource::new(move || {
        let selected_project = selected_project.get();
        let cache_key = selected_project_page_cache_key("codex", selected_project.as_deref());
        async move { (cache_key, load_codex_status_page(selected_project).await) }
    });
    refetch_on_live_event(page, codex_event_matches);

    view! {
        <Title text="Codex automation"/>
        {cached_page_view(
            move || selected_project_page_cache_key(
                "codex",
                selected_project_for_cache.get().as_deref(),
            ),
            page,
            codex_status_content,
            cache_codex_status_page,
            codex_status_page_from_cache,
        )}
    }
}

#[server(prefix = "/leptos")]
async fn load_codex_status_page(
    selected_project: Option<String>,
) -> Result<CodexStatusPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    page_data::codex_status_page_data(
        &state.store,
        &state.automation_controller,
        codex_status,
        selected_project.as_deref(),
    )
    .await
    .map_err(|err| ServerFnError::new(err.to_string()))
}

#[component]
pub fn PageItem() -> impl IntoView {
    let params = use_params_map();
    let project = params.read_untracked().get("project");
    let item_id = params
        .read_untracked()
        .get("item_id")
        .and_then(|value| value.parse::<i64>().ok());
    let cache_key = entity_page_cache_key("item", project.as_deref(), item_id);
    let project_for_loader = project.clone();
    let project_for_events = project;
    let cache_key_for_loader = cache_key.clone();
    let cache_key_for_view = cache_key;
    let api_base_url = api_base_url();
    let page = LocalResource::new(move || {
        let cache_key = cache_key_for_loader.clone();
        let project = project_for_loader.clone();
        let api_base_url = api_base_url.clone();
        async move {
            (
                cache_key,
                load_item_page(project, item_id, api_base_url).await,
            )
        }
    });
    refetch_on_live_event(page, move |event| {
        item_event_matches(event, project_for_events.clone(), item_id)
    });

    view! {
        <Title text="Patchbay"/>
        {cached_page_view(
            move || cache_key_for_view.clone(),
            page,
            item_content,
            cache_item_page,
            item_page_from_cache,
        )}
    }
}

#[server(prefix = "/leptos")]
async fn load_item_page(
    project: Option<String>,
    item_id: Option<i64>,
    api_base_url: String,
) -> Result<ItemPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    match (project, item_id) {
        (Some(project), Some(item_id)) => page_data::item_page_data(
            &state.store,
            &state.automation_controller,
            &project,
            item_id,
            api_base_url,
            codex_status,
        )
        .await
        .map_err(|err| ServerFnError::new(err.to_string())),
        _ => Err(ServerFnError::new("Missing item route parameters")),
    }
}

#[component]
pub fn PageRunLog() -> impl IntoView {
    let params = use_params_map();
    let project = params.read_untracked().get("project");
    let run_id = params
        .read_untracked()
        .get("run_id")
        .and_then(|value| value.parse::<i64>().ok());
    let cache_key = entity_page_cache_key("run-log", project.as_deref(), run_id);
    let project_for_loader = project.clone();
    let project_for_events = project;
    let cache_key_for_loader = cache_key.clone();
    let cache_key_for_view = cache_key;
    let page = LocalResource::new(move || {
        let cache_key = cache_key_for_loader.clone();
        let project = project_for_loader.clone();
        async move { (cache_key, load_run_log_page(project, run_id).await) }
    });
    refetch_on_live_event(page, move |event| {
        run_log_event_matches(event, project_for_events.clone(), run_id)
    });

    view! {
        <Title text="Run log"/>
        {cached_page_view(
            move || cache_key_for_view.clone(),
            page,
            run_log_content,
            cache_run_log_page,
            run_log_page_from_cache,
        )}
    }
}

#[server(prefix = "/leptos")]
async fn load_run_log_page(
    project: Option<String>,
    run_id: Option<i64>,
) -> Result<RunLogPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    match (project, run_id) {
        (Some(project), Some(run_id)) => page_data::run_log_page_data(
            &state.store,
            &state.sessions,
            &state.automation_controller,
            &project,
            run_id,
            codex_status,
        )
        .await
        .map_err(|err| ServerFnError::new(err.to_string())),
        _ => Err(ServerFnError::new("Missing run log route parameters")),
    }
}

#[component]
pub fn PageApiDocs() -> impl IntoView {
    let selected_project = selected_project_signal();
    let selected_project_for_cache = selected_project;
    let page = LocalResource::new(move || {
        let selected_project = selected_project.get();
        let cache_key = selected_project_page_cache_key("api-docs", selected_project.as_deref());
        async move { (cache_key, load_api_docs_page(selected_project).await) }
    });
    refetch_on_live_event(page, api_docs_event_matches);

    view! {
        <Title text="Patchbay API"/>
        {cached_page_view(
            move || selected_project_page_cache_key(
                "api-docs",
                selected_project_for_cache.get().as_deref(),
            ),
            page,
            api_docs_content,
            cache_api_docs_page,
            api_docs_page_from_cache,
        )}
    }
}

#[server(prefix = "/leptos")]
async fn load_api_docs_page(
    selected_project: Option<String>,
) -> Result<ApiDocsPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    page_data::api_docs_page_data(
        &state.store,
        &state.automation_controller,
        codex_status,
        selected_project.as_deref(),
    )
    .await
    .map_err(|err| ServerFnError::new(err.to_string()))
}

#[component]
pub fn PageError() -> impl IntoView {
    let message = error_message_from_query();

    view! {
        <Title text="Error"/>
        {error_content(message.unwrap_or_else(|| "An error occurred.".to_owned()))}
    }
}

#[component]
pub fn PageErr404() -> impl IntoView {
    #[cfg(feature = "ssr")]
    if let Some(options) = use_context::<ResponseOptions>() {
        options.overwrite(ResponseParts {
            status: Some(axum::http::StatusCode::NOT_FOUND),
            ..Default::default()
        });
    }

    view! {
        <Title text="Not found"/>
        {error_content("Page not found.".to_owned())}
    }
}

fn selected_project_signal() -> Memo<Option<String>> {
    let query = use_query_map();
    Memo::new(move |_| query.read().get("project"))
}

fn selected_project_page_cache_key(page: &str, selected_project: Option<&str>) -> String {
    format!("{page}:project={}", selected_project.unwrap_or_default())
}

fn entity_page_cache_key(page: &str, project: Option<&str>, id: Option<i64>) -> String {
    let id = id.map(|id| id.to_string()).unwrap_or_default();
    format!("{page}:project={}:id={id}", project.unwrap_or_default())
}

fn cache_board_page(page: BoardPage) -> CachedRoutePage {
    CachedRoutePage::Board(Box::new(page))
}

fn board_page_from_cache(page: &CachedRoutePage) -> Option<BoardPage> {
    match page {
        CachedRoutePage::Board(page) => Some(page.as_ref().clone()),
        _ => None,
    }
}

fn cache_projects_page(page: ProjectsPage) -> CachedRoutePage {
    CachedRoutePage::Projects(Box::new(page))
}

fn projects_page_from_cache(page: &CachedRoutePage) -> Option<ProjectsPage> {
    match page {
        CachedRoutePage::Projects(page) => Some(page.as_ref().clone()),
        _ => None,
    }
}

fn cache_triggers_page(page: TriggersPage) -> CachedRoutePage {
    CachedRoutePage::Triggers(Box::new(page))
}

fn triggers_page_from_cache(page: &CachedRoutePage) -> Option<TriggersPage> {
    match page {
        CachedRoutePage::Triggers(page) => Some(page.as_ref().clone()),
        _ => None,
    }
}

fn cache_runs_page(page: RunsPage) -> CachedRoutePage {
    CachedRoutePage::Runs(Box::new(page))
}

fn runs_page_from_cache(page: &CachedRoutePage) -> Option<RunsPage> {
    match page {
        CachedRoutePage::Runs(page) => Some(page.as_ref().clone()),
        _ => None,
    }
}

fn cache_codex_status_page(page: CodexStatusPage) -> CachedRoutePage {
    CachedRoutePage::Codex(Box::new(page))
}

fn codex_status_page_from_cache(page: &CachedRoutePage) -> Option<CodexStatusPage> {
    match page {
        CachedRoutePage::Codex(page) => Some(page.as_ref().clone()),
        _ => None,
    }
}

fn cache_item_page(page: ItemPage) -> CachedRoutePage {
    CachedRoutePage::Item(Box::new(page))
}

fn item_page_from_cache(page: &CachedRoutePage) -> Option<ItemPage> {
    match page {
        CachedRoutePage::Item(page) => Some(page.as_ref().clone()),
        _ => None,
    }
}

fn cache_run_log_page(page: RunLogPage) -> CachedRoutePage {
    CachedRoutePage::RunLog(Box::new(page))
}

fn run_log_page_from_cache(page: &CachedRoutePage) -> Option<RunLogPage> {
    match page {
        CachedRoutePage::RunLog(page) => Some(page.as_ref().clone()),
        _ => None,
    }
}

fn cache_api_docs_page(page: ApiDocsPage) -> CachedRoutePage {
    CachedRoutePage::ApiDocs(Box::new(page))
}

fn api_docs_page_from_cache(page: &CachedRoutePage) -> Option<ApiDocsPage> {
    match page {
        CachedRoutePage::ApiDocs(page) => Some(page.as_ref().clone()),
        _ => None,
    }
}

fn codex_event_matches(event: &UiEvent) -> bool {
    matches!(
        event,
        UiEvent::CodexStatusChanged { .. }
            | UiEvent::AgentToolChanged { .. }
            | UiEvent::ProjectListChanged { .. }
            | UiEvent::ProjectChanged { .. }
            | UiEvent::AutomationChanged { .. }
    )
}

fn api_docs_event_matches(event: &UiEvent) -> bool {
    matches!(
        event,
        UiEvent::ProjectListChanged { .. }
            | UiEvent::ProjectChanged { .. }
            | UiEvent::CodexStatusChanged { .. }
    )
}

fn runs_page_event_matches(event: &UiEvent) -> bool {
    matches!(
        event,
        UiEvent::ProjectListChanged { .. }
            | UiEvent::ProjectChanged { .. }
            | UiEvent::CodexStatusChanged { .. }
    )
}

fn item_event_matches(event: &UiEvent, project: Option<String>, item_id: Option<i64>) -> bool {
    if !event_scopes_named_project(event, project.as_deref()) {
        return false;
    }
    match event {
        UiEvent::ProjectListChanged { .. }
        | UiEvent::ProjectChanged { .. }
        | UiEvent::AutomationChanged { .. }
        | UiEvent::CodexStatusChanged { .. }
        | UiEvent::AgentToolChanged { .. } => true,
        UiEvent::WorkItemChanged {
            item_id: changed_item_id,
            ..
        }
        | UiEvent::CommentChanged {
            item_id: changed_item_id,
            ..
        } => Some(*changed_item_id) == item_id,
        UiEvent::AgentRunChanged {
            item_id: Some(changed_item_id),
            ..
        }
        | UiEvent::AgentOutputChanged {
            item_id: Some(changed_item_id),
            ..
        } => Some(*changed_item_id) == item_id,
        UiEvent::AgentRunChanged { item_id: None, .. }
        | UiEvent::AgentOutputChanged { item_id: None, .. }
        | UiEvent::SystemPromptChanged { .. }
        | UiEvent::MemoryChanged { .. }
        | UiEvent::SwimLaneChanged { .. } => false,
        UiEvent::WorkItemStateChanged { .. } => true,
    }
}

fn run_log_event_matches(event: &UiEvent, project: Option<String>, run_id: Option<i64>) -> bool {
    if !event_scopes_named_project(event, project.as_deref()) {
        return false;
    }
    match event {
        UiEvent::AgentRunChanged {
            run_id: changed_run_id,
            ..
        }
        | UiEvent::AgentOutputChanged {
            run_id: changed_run_id,
            ..
        } => Some(*changed_run_id) == run_id,
        UiEvent::ProjectListChanged { .. }
        | UiEvent::ProjectChanged { .. }
        | UiEvent::AutomationChanged { .. }
        | UiEvent::CodexStatusChanged { .. }
        | UiEvent::AgentToolChanged { .. } => true,
        UiEvent::WorkItemChanged { .. }
        | UiEvent::CommentChanged { .. }
        | UiEvent::SystemPromptChanged { .. }
        | UiEvent::MemoryChanged { .. }
        | UiEvent::SwimLaneChanged { .. }
        | UiEvent::WorkItemStateChanged { .. } => false,
    }
}

fn error_message_from_query() -> Option<String> {
    use_query_map().read_untracked().get("message")
}

fn api_base_url() -> String {
    format!("{}/api", request_origin())
}

#[cfg(feature = "ssr")]
fn request_origin() -> String {
    use axum::http::header;

    use_context::<axum::http::request::Parts>()
        .map(|parts| {
            let scheme = header_value(&parts.headers, "x-forwarded-proto").unwrap_or("http");
            let host = header_value(&parts.headers, "x-forwarded-host")
                .or_else(|| header_value(&parts.headers, header::HOST.as_str()))
                .unwrap_or("127.0.0.1:4000");
            format!("{scheme}://{host}")
        })
        .unwrap_or_else(|| "http://127.0.0.1:4000".to_owned())
}

#[cfg(feature = "ssr")]
fn header_value<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(not(feature = "ssr"))]
fn request_origin() -> String {
    window()
        .location()
        .origin()
        .unwrap_or_else(|_| "http://127.0.0.1:4000".to_owned())
}

fn page_loading() -> impl IntoView {
    view! {
        <main class="page-shell">
            <p class="muted">"Loading..."</p>
        </main>
    }
}

#[derive(Clone)]
enum ResilientPage<T> {
    Content(T),
    InitialError(String),
}

fn cached_page_view<T>(
    cache_key: impl Fn() -> String + Clone + 'static,
    resource: LocalResource<(String, Result<T, ServerFnError>)>,
    content: impl Fn(T) -> AnyView + Copy + Send + 'static,
    into_cached_page: impl Fn(T) -> CachedRoutePage + Copy + 'static,
    from_cached_page: impl Fn(&CachedRoutePage) -> Option<T> + Copy + 'static,
) -> impl IntoView
where
    T: Clone + Send + Sync + 'static,
{
    let cache = use_context::<RoutePageCacheContext>();
    let initial_cache_key = cache_key();
    let initial_page = cached_route_page(cache, &initial_cache_key, from_cached_page);
    let has_initial_page = initial_page.is_some();
    let (displayed_cache_key, set_displayed_cache_key) = signal(initial_cache_key);
    let (displayed_page, set_displayed_page) = signal(initial_page.map(ResilientPage::Content));
    let (skip_next_visible_update, set_skip_next_visible_update) = signal(has_initial_page);
    notify_page_resource_errors(resource, move || {
        displayed_page.with_untracked(|page| matches!(page, Some(ResilientPage::Content(_))))
    });

    let cache_key_for_route_change = cache_key.clone();
    Effect::new(move |_| {
        let key = cache_key_for_route_change();
        if displayed_cache_key.with_untracked(|displayed| displayed != &key) {
            let cached = cached_route_page(cache, &key, from_cached_page);
            if let Some(cached) = cached {
                set_displayed_page.set(Some(ResilientPage::Content(cached)));
                set_skip_next_visible_update.set(true);
            } else {
                set_skip_next_visible_update.set(false);
            }
            set_displayed_cache_key.set(key);
        }
    });

    let cache_key_for_resource = cache_key;
    Effect::new(move |_| match resource.get() {
        Some((loaded_key, Ok(page))) => {
            if let Some(cache) = cache {
                let cached = into_cached_page(page.clone());
                cache.set_pages.update(|pages| {
                    pages.insert(loaded_key.clone(), cached);
                });
            }
            if loaded_key == cache_key_for_resource() {
                let should_skip_visible_update = skip_next_visible_update
                    .with_untracked(|skip| *skip)
                    && displayed_page
                        .with_untracked(|page| matches!(page, Some(ResilientPage::Content(_))));
                if should_skip_visible_update {
                    set_skip_next_visible_update.set(false);
                } else {
                    set_displayed_cache_key.set(loaded_key);
                    set_displayed_page.set(Some(ResilientPage::Content(page)));
                }
            }
        }
        Some((loaded_key, Err(err)))
            if loaded_key == cache_key_for_resource()
                && displayed_page.with_untracked(Option::is_none) =>
        {
            set_displayed_page.set(Some(ResilientPage::InitialError(err.to_string())));
        }
        Some((_, Err(_))) | None => {}
    });

    move || match displayed_page.get() {
        Some(ResilientPage::Content(page)) => content(page),
        Some(ResilientPage::InitialError(message)) => error_content(message),
        None => page_loading().into_any(),
    }
}

fn cached_route_page<T>(
    cache: Option<RoutePageCacheContext>,
    key: &str,
    from_cached_page: impl Fn(&CachedRoutePage) -> Option<T>,
) -> Option<T> {
    cache.and_then(|cache| {
        cache
            .pages
            .with_untracked(|pages| pages.get(key).and_then(from_cached_page))
    })
}

fn notify_page_resource_errors<T>(
    resource: LocalResource<(String, Result<T, ServerFnError>)>,
    should_notify: impl Fn() -> bool + Copy + 'static,
) where
    T: Clone + Send + Sync + 'static,
{
    #[cfg(not(feature = "ssr"))]
    let toasts = use_context::<Toasts>();
    let (last_notified_error, set_last_notified_error) = signal(None::<String>);
    Effect::new(move |_| {
        if let Some((_, result)) = resource.get() {
            match result {
                Ok(_) => set_last_notified_error.set(None),
                Err(err) if should_notify() => {
                    let message = err.to_string();
                    let already_notified = last_notified_error
                        .with_untracked(|last| last.as_deref() == Some(message.as_str()));
                    if !already_notified {
                        #[cfg(not(feature = "ssr"))]
                        show_request_error_toast(toasts, message.clone());
                        #[cfg(feature = "ssr")]
                        show_request_error_toast(message.clone());
                        set_last_notified_error.set(Some(message));
                    }
                }
                Err(_) => {}
            }
        }
    });
}

fn notify_resource_errors<T>(
    resource: LocalResource<Result<T, ServerFnError>>,
    should_notify: impl Fn() -> bool + Copy + 'static,
) where
    T: Clone + Send + Sync + 'static,
{
    #[cfg(not(feature = "ssr"))]
    let toasts = use_context::<Toasts>();
    let (last_notified_error, set_last_notified_error) = signal(None::<String>);
    Effect::new(move |_| {
        if let Some(result) = resource.get() {
            match result {
                Ok(_) => set_last_notified_error.set(None),
                Err(err) if should_notify() => {
                    let message = err.to_string();
                    let already_notified = last_notified_error
                        .with_untracked(|last| last.as_deref() == Some(message.as_str()));
                    if !already_notified {
                        #[cfg(not(feature = "ssr"))]
                        show_request_error_toast(toasts, message.clone());
                        #[cfg(feature = "ssr")]
                        show_request_error_toast(message.clone());
                        set_last_notified_error.set(Some(message));
                    }
                }
                Err(_) => {}
            }
        }
    });
}

#[cfg(not(feature = "ssr"))]
fn show_request_error_toast(toasts: Option<Toasts>, message: String) {
    let Some(toasts) = toasts else {
        return;
    };
    let body = message;
    toasts.push(Toast {
        id: Uuid::new_v4(),
        created_at: OffsetDateTime::now_utc(),
        variant: ToastVariant::Error,
        header: ViewFn::from(|| "Request failed"),
        body: ViewFn::from(move || body.clone()),
        timeout: ToastTimeout::DefaultDelay,
    });
}

#[cfg(feature = "ssr")]
fn show_request_error_toast(_message: String) {}

fn background_form_submit(
    reset_on_success: bool,
) -> impl Fn(leptos::ev::SubmitEvent) + Clone + 'static {
    #[cfg(not(feature = "ssr"))]
    {
        let toasts = use_context::<Toasts>();
        move |event: leptos::ev::SubmitEvent| {
            event.prevent_default();
            let Some(form) = event.current_target().or_else(|| event.target()) else {
                show_request_error_toast(toasts.clone(), "Missing submitted form".to_owned());
                return;
            };
            let form = wasm_bindgen::JsValue::from(form);
            let toasts = toasts.clone();
            leptos::task::spawn_local(async move {
                if let Err(message) = submit_background_form(form, reset_on_success).await {
                    show_request_error_toast(toasts, message);
                }
            });
        }
    }
    #[cfg(feature = "ssr")]
    {
        let _ = reset_on_success;
        move |_event: leptos::ev::SubmitEvent| {}
    }
}

#[cfg(not(feature = "ssr"))]
async fn submit_background_form(
    form: wasm_bindgen::JsValue,
    reset_on_success: bool,
) -> Result<(), String> {
    match js_submit_background_form(form, reset_on_success).await {
        Ok(message) => {
            let message = message.as_string().unwrap_or_default();
            if message.is_empty() {
                Ok(())
            } else {
                Err(message)
            }
        }
        Err(err) => Err(js_error_message(err)),
    }
}

#[cfg(not(feature = "ssr"))]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export async function patchbaySubmitBackgroundForm(form, resetOnSuccess) {
  if (!(form instanceof HTMLFormElement)) {
    return 'Missing submitted form';
  }

  const response = await fetch(form.action, {
    method: (form.method || 'POST').toUpperCase(),
    body: new URLSearchParams(new FormData(form)),
    headers: { 'x-patchbay-background': 'true' },
  });

  if (!response.ok) {
    const body = await response.text();
    return body || `${response.status} ${response.statusText}`;
  }

  if (resetOnSuccess) {
    form.reset();
  }

  return '';
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(catch, js_name = patchbaySubmitBackgroundForm)]
    async fn js_submit_background_form(
        form: wasm_bindgen::JsValue,
        reset_on_success: bool,
    ) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>;
}

fn board_content(page: BoardPage) -> AnyView {
    let BoardPage {
        projects,
        active_project_names,
        selected_project,
        selected_project_view,
        settings,
        workspace_editors,
        system_prompt_events,
        memory_events,
        automation_status,
        automation_running,
        items,
        swim_lanes,
        work_item_states,
        label_suggestions,
        misconfigured_item_count,
        api_base_url,
        codex_status,
        runtime,
    } = page;

    if let (Some(project), Some(project_view), Some(settings), Some(automation_status)) = (
        selected_project.clone(),
        selected_project_view,
        settings,
        automation_status,
    ) {
        let topbar_settings = settings.clone();
        let (auto_commit, set_auto_commit) = signal(settings.auto_commit);
        let topbar = top_bar(
            projects,
            active_project_names,
            Some(project.clone()),
            ActivePage::Board,
            Some(TopBarAutomation {
                project: project.clone(),
                running: automation_running || automation_status.running_runs > 0,
                workspace_mode: topbar_settings.workspace_mode,
                auto_commit,
                set_auto_commit,
            }),
            codex_status.clone(),
        );
        let page_title = project_view.display_name.clone();
        let board_return_to = format!("/?project={}", encode_path(&project));
        let project_workspace = project_workspace_panel(
            &project,
            &project_view,
            workspace_editors.clone(),
            board_return_to.clone(),
        );
        let (show_create_item_modal, set_show_create_item_modal) = signal(false);
        let initial_create_item_state_options =
            state_options_from_project_states(&work_item_states);
        let work_item_states_context = provide_work_item_states_context(work_item_states);
        let initial_create_item_state =
            default_state_identifier(&initial_create_item_state_options);
        let (create_item_state, set_create_item_state) = signal(initial_create_item_state);
        let (create_item_state_options, set_create_item_state_options) =
            signal(initial_create_item_state_options);
        let create_item_label_suggestions = Signal::derive(move || label_suggestions.clone());
        let has_create_item_states = Memo::new(move |_| {
            !state_options_from_project_states(&work_item_states_context.states.get()).is_empty()
        });
        let open_create_item = Callback::new(move |request: CreateItemOpenRequest| {
            let states = work_item_states_context.states.get_untracked();
            let options = state_options_for_open_request(&states, &request);
            if options.is_empty() {
                return;
            }
            set_create_item_state.set(default_state_identifier(&options));
            set_create_item_state_options.set(options);
            set_show_create_item_modal.set(true);
        });
        let board = view! {
            <LiveBoardItems
                project=project.clone()
                initial_items=items
                initial_swim_lanes=swim_lanes
                initial_misconfigured_item_count=misconfigured_item_count
                open_create_item=open_create_item
            />
        };
        let admin_project_id = project_view.id;
        let create_item = create_item_modal(
            api_base_url.clone(),
            admin_project_id,
            show_create_item_modal,
            set_show_create_item_modal,
            create_item_state_options,
            create_item_state,
            create_item_label_suggestions,
        );
        let work_items_api_base_url = api_base_url.clone();
        let project_settings = project_settings_view(
            &project,
            project_view,
            settings,
            system_prompt_events,
            memory_events,
            auto_commit,
            set_auto_commit,
        );
        let maintenance = maintenance_view(&project);
        let runtime = runtime_panel(runtime, format!("/?project={}", encode_path(&project)));

        view! {
            <div>
                {topbar}
                <main class="page-shell">
                    <section class="board-toolbar">
                        <div class="board-heading">
                            <h1>{page_title}</h1>
                        </div>
                        <button
                            type="button"
                            disabled=move || !has_create_item_states.get()
                            on:click=move |_| {
                                open_create_item.run(CreateItemOpenRequest::AnyState)
                            }
                        >
                            "New item"
                        </button>
                    </section>
                    <section class="workspace-panel panel">
                        <div class="panel-heading">
                            <h2>"Workspace"</h2>
                        </div>
                        {project_workspace}
                    </section>
                    {board}
                    {create_item}
                    <WorkItemsPanel
                        api_base_url=work_items_api_base_url
                        project=project.clone()
                        project_id=admin_project_id
                    />
                    {project_settings}
                    {runtime}
                    {maintenance}
                </main>
            </div>
        }
        .into_any()
    } else {
        let topbar = top_bar(
            projects,
            active_project_names,
            selected_project,
            ActivePage::Board,
            None,
            codex_status,
        );
        view! {
            <div>
                {topbar}
                <main class="empty-state">
                    <h1>"Choose a project"</h1>
                    <a class="button-link" href="/projects">"Projects"</a>
                    {runtime_panel(runtime, "/".to_owned())}
                </main>
            </div>
        }
        .into_any()
    }
}

fn runs_content(page: RunsPage) -> AnyView {
    let RunsPage {
        projects,
        active_project_names,
        selected_project,
        automation_status,
        automation_running,
        run_sessions,
        workspace_editors,
        codex_status,
    } = page;
    let topbar = top_bar(
        projects,
        active_project_names,
        selected_project.clone(),
        ActivePage::Runs,
        None,
        codex_status,
    );

    if let (Some(project), Some(automation_status)) = (selected_project, automation_status) {
        view! {
            <div>
                {topbar}
                <main class="page-shell runs-page">
                    <section class="page-heading">
                        <h1>"Runs"</h1>
                    </section>
                    <LiveRunsSection
                        project=project
                        initial_status=automation_status
                        initial_running=automation_running
                        initial_run_sessions=run_sessions
                        workspace_editors=workspace_editors
                    />
                </main>
            </div>
        }
        .into_any()
    } else {
        view! {
            <div>
                {topbar}
                <main class="empty-state">
                    <h1>"Choose a project"</h1>
                    <a class="button-link" href="/projects">"Projects"</a>
                </main>
            </div>
        }
        .into_any()
    }
}

fn projects_content(page: ProjectsPage) -> AnyView {
    view! {
        <ProjectsContent
            projects=page.projects
            active_project_names=page.active_project_names
            selected_project=page.selected_project
            api_base_url=page.api_base_url
            codex_status=page.codex_status
        />
    }
    .into_any()
}

fn triggers_content(page: TriggersPage) -> AnyView {
    let TriggersPage {
        projects,
        active_project_names,
        selected_project,
        selected_project_view,
        workspace_editors,
        api_base_url,
        codex_status,
    } = page;
    let topbar = top_bar(
        projects,
        active_project_names,
        selected_project.clone(),
        ActivePage::Triggers,
        None,
        codex_status,
    );

    if let (Some(project), Some(project_view)) = (selected_project, selected_project_view) {
        let (consumer_context, set_consumer_context) = signal(None::<CrudInstanceContext>);
        let (producer_context, set_producer_context) = signal(None::<CrudInstanceContext>);
        let selected_trigger_id = Memo::new(move |_| {
            consumer_context
                .get()
                .and_then(selected_trigger_id_from_context)
                .or_else(|| {
                    producer_context
                        .get()
                        .and_then(selected_trigger_id_from_context)
                })
        });
        let consuming_triggers = automation_triggers_crudkit_instance(
            api_base_url.clone(),
            project.clone(),
            project_view.id,
            AutomationTableKind::Consuming,
            Callback::new(move |context| set_consumer_context.set(Some(context))),
        );
        let producing_triggers = automation_triggers_crudkit_instance(
            api_base_url,
            project.clone(),
            project_view.id,
            AutomationTableKind::Producing,
            Callback::new(move |context| set_producer_context.set(Some(context))),
        );
        let trigger_runs = trigger_runs_panel(
            project.clone(),
            selected_trigger_id,
            workspace_editors.clone(),
        );
        view! {
            <div>
                {topbar}
                <main class="page-shell triggers-page">
                    <section class="page-heading">
                        <h1>"Automation"</h1>
                    </section>
                    <section class="automation-triggers panel">
                        <div class="panel-heading">
                            <h2>"Work-consuming automations"</h2>
                        </div>
                        <div class="crudkit-automation-triggers" data-crudkit-leptos="automation-triggers">
                            {consuming_triggers}
                        </div>
                    </section>
                    <section class="automation-triggers panel">
                        <div class="panel-heading">
                            <h2>"Work-producing automations"</h2>
                        </div>
                        <div class="crudkit-automation-triggers" data-crudkit-leptos="automation-triggers">
                            {producing_triggers}
                        </div>
                    </section>
                    {trigger_runs}
                </main>
            </div>
        }
        .into_any()
    } else {
        view! {
            <div>
                {topbar}
                <main class="empty-state">
                    <h1>"Choose a project"</h1>
                    <a class="button-link" href="/projects">"Projects"</a>
                </main>
            </div>
        }
        .into_any()
    }
}

#[component]
fn ProjectsContent(
    projects: Vec<ProjectView>,
    active_project_names: Vec<String>,
    selected_project: Option<String>,
    api_base_url: String,
    codex_status: CodexAppServerStatusView,
) -> impl IntoView + 'static {
    let selected_project_view = selected_project
        .as_ref()
        .and_then(|project| projects.iter().find(|candidate| candidate.name == *project))
        .cloned()
        .or_else(|| projects.first().cloned());
    let topbar = top_bar(
        projects.clone(),
        active_project_names,
        selected_project.clone(),
        ActivePage::Projects,
        None,
        codex_status,
    );
    let query = use_query_map();
    let edit_swim_lane_id = query
        .read_untracked()
        .get("edit_swim_lane")
        .and_then(|value| value.parse().ok());
    let project_authoring = selected_project_view.as_ref().map(|project_view| {
        let project_name = project_view.name.clone();
        let project_id = project_view.id;
        view! {
            <WorkItemStatesPanel
                api_base_url=api_base_url.clone()
                project=project_name.clone()
                project_id=project_id
            />
            <SwimLanesPanel
                api_base_url=api_base_url.clone()
                project=project_name
                project_id=project_id
                edit_lane_id=edit_swim_lane_id
            />
        }
    });

    view! {
        <div>
            {topbar}
            <main class="page-shell projects-page">
                <section class="page-heading">
                    <h1>"Projects"</h1>
                </section>
                {projects_panel(api_base_url)}
                {project_authoring}
            </main>
        </div>
    }
}

fn codex_status_content(page: CodexStatusPage) -> AnyView {
    let CodexStatusPage {
        projects,
        active_project_names,
        selected_project,
        codex_status,
    } = page;
    let return_to = selected_project
        .as_deref()
        .map(|project| format!("/codex?project={}", encode_path(project)))
        .unwrap_or_else(|| "/codex".to_owned());
    let topbar = top_bar(
        projects,
        active_project_names,
        selected_project,
        ActivePage::Codex,
        None,
        codex_status.clone(),
    );

    view! {
        <div>
            {topbar}
            <main class="page-shell codex-page">
                <section class="page-heading">
                    <h1>"Codex automation"</h1>
                </section>
                {codex_status_panel(&codex_status, return_to)}
            </main>
        </div>
    }
    .into_any()
}

fn codex_status_panel(status: &CodexAppServerStatusView, return_to: String) -> AnyView {
    let status_class = if status.usable {
        "codex-status-ready"
    } else if status.available {
        "codex-status-blocked"
    } else {
        "codex-status-unavailable"
    };
    let heading = if status.usable {
        "Codex automation ready"
    } else if status.available {
        "Codex automation blocked"
    } else {
        "Codex app-server unavailable"
    };
    let badge = if status.usable {
        "Ready"
    } else if status.available {
        "Blocked"
    } else {
        "Unavailable"
    };
    let binary = status
        .binary_path
        .clone()
        .unwrap_or_else(|| "not resolved".to_owned());
    let auth_method = status
        .auth_method
        .as_deref()
        .map(auth_method_label)
        .unwrap_or_else(|| "Not signed in".to_owned());
    let account = status
        .account_label
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let plan = status
        .plan_type
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let payment = status
        .payment_model
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let requires_auth = status
        .requires_openai_auth
        .map(|value| if value { "yes" } else { "no" })
        .unwrap_or("unknown");
    let preconditions = status
        .preconditions
        .clone()
        .into_iter()
        .map(|precondition| {
            view! {
                <li>
                    <span class=if precondition.ok {
                        "check-state ok"
                    } else {
                        "check-state failed"
                    }>
                        {if precondition.ok { "OK" } else { "Fail" }}
                    </span>
                    <span>
                        <strong>{precondition.name}</strong>
                        <span>{precondition.message}</span>
                    </span>
                </li>
            }
        })
        .collect::<Vec<_>>();
    let rate_limits = if status.rate_limits.is_empty() {
        view! { <p class="muted">"No rate-limit details reported."</p> }.into_any()
    } else {
        let limits = status
            .rate_limits
            .iter()
            .map(rate_limit_view)
            .collect::<Vec<_>>();
        view! { <div class="codex-rate-limits">{limits}</div> }.into_any()
    };
    let usage = status
        .usage_summary
        .as_ref()
        .map(usage_summary_view)
        .unwrap_or_else(|| {
            view! { <p class="muted">"No token usage summary reported."</p> }.into_any()
        });
    let warnings = if status.warnings.is_empty() {
        ().into_any()
    } else {
        let warnings = status
            .warnings
            .clone()
            .into_iter()
            .map(|warning| view! { <li>{warning}</li> })
            .collect::<Vec<_>>();
        view! { <ul class="codex-status-warnings">{warnings}</ul> }.into_any()
    };
    let install_prompt = (!status.available).then(|| {
        view! { <p class="codex-install-prompt">{status.install_prompt.clone()}</p> }
    });
    let auth_setup = status.auth_setup.clone().map(codex_auth_setup_view);
    let can_logout = status.available
        && status.auth_method.as_deref() != Some("apiKey")
        && (status.signed_in || status.auth_setup.is_some());
    let return_to_for_refresh = return_to.clone();
    let return_to_for_logout = return_to;
    let logout_action = can_logout.then(|| {
        view! {
            <form method="post" action="/codex/logout">
                <input type="hidden" name="return_to" value=return_to_for_logout/>
                <button type="submit" class="danger">"Log out"</button>
            </form>
        }
    });

    view! {
        <section class=format!("codex-status-panel {status_class}")>
            <div class="codex-status-header">
                <div>
                    <h2>{heading}</h2>
                    <p>{status.message.clone()}</p>
                    {install_prompt}
                </div>
                <div class="codex-status-actions">
                    <span class="codex-status-badge">{badge}</span>
                    <form method="post" action="/agent-tools/discover">
                        <input type="hidden" name="return_to" value=return_to_for_refresh/>
                        <button type="submit" class="secondary">"Refresh"</button>
                    </form>
                    {logout_action}
                </div>
            </div>
            {auth_setup}
            <div class="codex-status-grid">
                <div>
                    <span>"Binary"</span>
                    <code>{binary}</code>
                </div>
                <div>
                    <span>"Auth"</span>
                    <strong>{auth_method}</strong>
                </div>
                <div>
                    <span>"Account"</span>
                    <strong>{account}</strong>
                </div>
                <div>
                    <span>"OpenAI auth required"</span>
                    <strong>{requires_auth}</strong>
                </div>
                <div>
                    <span>"Payment"</span>
                    <strong>{payment}</strong>
                </div>
                <div>
                    <span>"Plan"</span>
                    <strong>{plan}</strong>
                </div>
                <div>
                    <span>"Checked"</span>
                    <strong>{status.checked_at.clone()}</strong>
                </div>
            </div>
            <div class="codex-status-columns">
                <div>
                    <h3>"Preconditions"</h3>
                    <ul class="codex-preconditions">{preconditions}</ul>
                </div>
                <div>
                    <h3>"Limits"</h3>
                    {rate_limits}
                </div>
                <div>
                    <h3>"Usage"</h3>
                    {usage}
                </div>
            </div>
            {warnings}
        </section>
    }
    .into_any()
}

fn codex_auth_setup_view(setup: CodexAuthSetupView) -> AnyView {
    let command = setup.login_command.clone();
    let command_for_copy = command.clone();
    let home_for_copy = setup.codex_home_path.clone();
    let (copy_message, set_copy_message) = signal(None::<String>);

    view! {
        <div class="codex-auth-guide">
            <div class="codex-auth-guide-main">
                <div>
                    <h3>"Sign in to Codex"</h3>
                    <p>
                        "Run this command in a terminal. It writes credentials into Patchbay's managed Codex home."
                    </p>
                </div>
                <div class="codex-auth-actions">
                    <button
                        type="button"
                        class="secondary"
                        on:click=move |_| {
                            copy_workspace_text(
                                command_for_copy.clone(),
                                "Copied login command",
                                set_copy_message,
                            );
                        }
                    >
                        "Copy command"
                    </button>
                    <button
                        type="button"
                        class="secondary"
                        on:click=move |_| {
                            copy_workspace_text(
                                home_for_copy.clone(),
                                "Copied Codex home",
                                set_copy_message,
                            );
                        }
                    >
                        "Copy home"
                    </button>
                    {move || {
                        copy_message
                            .get()
                            .map(|message| view! { <span class="workspace-copy-status">{message}</span> })
                    }}
                </div>
            </div>
            <code class="codex-login-command">{command}</code>
            <div class="codex-auth-notes">
                <p>{setup.refresh_instruction}</p>
                <p>{setup.api_key_instruction}</p>
            </div>
        </div>
    }
    .into_any()
}

fn auth_method_label(method: &str) -> String {
    match method {
        "apiKey" => "API key".to_owned(),
        "chatgpt" => "ChatGPT".to_owned(),
        "amazonBedrock" => "Amazon Bedrock".to_owned(),
        method => method.to_owned(),
    }
}

fn rate_limit_view(limit: &CodexRateLimitView) -> AnyView {
    let lines = rate_limit_lines(limit)
        .into_iter()
        .map(|line| view! { <li>{line}</li> })
        .collect::<Vec<_>>();
    let reached = limit.reached_type.clone().map(|reached| {
        view! { <span class="check-state failed">{reached}</span> }
    });

    view! {
        <article class="codex-rate-limit">
            <div>
                <strong>{limit.label.clone()}</strong>
                {limit.plan_type.as_ref().map(|plan| view! {
                    <span class="muted">"plan " {plan.clone()}</span>
                })}
            </div>
            {reached}
            <ul>{lines}</ul>
        </article>
    }
    .into_any()
}

fn rate_limit_lines(limit: &CodexRateLimitView) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(line) = rate_window_line(
        "Primary",
        limit.primary_used_percent,
        limit.primary_window_minutes,
        limit.primary_resets_at.as_deref(),
    ) {
        lines.push(line);
    }
    if let Some(line) = rate_window_line(
        "Secondary",
        limit.secondary_used_percent,
        limit.secondary_window_minutes,
        limit.secondary_resets_at.as_deref(),
    ) {
        lines.push(line);
    }
    if let Some(remaining) = limit.individual_remaining_percent {
        let mut line = format!("{remaining}% individual budget remaining");
        if let (Some(used), Some(max)) = (&limit.individual_used, &limit.individual_limit) {
            line.push_str(&format!(" ({used} of {max})"));
        }
        if let Some(resets_at) = &limit.individual_resets_at {
            line.push_str(&format!(", resets {resets_at}"));
        }
        lines.push(line);
    }
    if limit.credits_balance.is_some()
        || limit.credits_has_credits.is_some()
        || limit.credits_unlimited.is_some()
    {
        let balance = limit
            .credits_balance
            .clone()
            .unwrap_or_else(|| "unknown".to_owned());
        let has_credits = limit
            .credits_has_credits
            .map(|value| if value { "yes" } else { "no" })
            .unwrap_or("unknown");
        let unlimited = limit
            .credits_unlimited
            .map(|value| if value { "yes" } else { "no" })
            .unwrap_or("unknown");
        lines.push(format!(
            "Credits balance {balance}; has credits {has_credits}; unlimited {unlimited}"
        ));
    }
    if lines.is_empty() {
        lines.push("No window details reported.".to_owned());
    }
    lines
}

fn rate_window_line(
    label: &str,
    used_percent: Option<i64>,
    window_minutes: Option<i64>,
    resets_at: Option<&str>,
) -> Option<String> {
    let used_percent = used_percent?;
    let mut line = format!("{label}: {used_percent}% used");
    if let Some(window_minutes) = window_minutes {
        line.push_str(&format!(" over {window_minutes} min"));
    }
    if let Some(resets_at) = resets_at {
        line.push_str(&format!(", resets {resets_at}"));
    }
    Some(line)
}

fn usage_summary_view(summary: &CodexUsageSummaryView) -> AnyView {
    let mut rows = Vec::new();
    if let Some(value) = summary.lifetime_tokens {
        rows.push(("Lifetime tokens", format_number(value)));
    }
    if let Some(value) = summary.peak_daily_tokens {
        rows.push(("Peak daily tokens", format_number(value)));
    }
    if let Some(value) = summary.current_streak_days {
        rows.push(("Current streak", format!("{value} days")));
    }
    if let Some(value) = summary.longest_streak_days {
        rows.push(("Longest streak", format!("{value} days")));
    }
    if let Some(value) = summary.longest_running_turn_seconds {
        rows.push(("Longest turn", format!("{value} sec")));
    }
    if rows.is_empty() {
        return view! { <p class="muted">"No token usage summary reported."</p> }.into_any();
    }
    let rows = rows
        .into_iter()
        .map(|(label, value)| {
            view! {
                <div>
                    <span>{label}</span>
                    <strong>{value}</strong>
                </div>
            }
        })
        .collect::<Vec<_>>();
    view! { <div class="codex-usage-summary">{rows}</div> }.into_any()
}

fn format_number(value: i64) -> String {
    let absolute = if value < 0 {
        -(value as i128)
    } else {
        value as i128
    };
    let mut chars = absolute.to_string().chars().rev().collect::<Vec<_>>();
    let mut formatted = String::new();
    for (index, ch) in chars.drain(..).enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    let mut formatted = formatted.chars().rev().collect::<String>();
    if value < 0 {
        formatted.insert(0, '-');
    }
    formatted
}

fn without_crudkit_field<F: TypeErasedField>(
    elements: Vec<Elem<F>>,
    field_name: &str,
) -> Vec<Elem<F>> {
    elements
        .into_iter()
        .filter_map(|element| match element {
            Elem::Field((field, _)) if field.name() == field_name => None,
            Elem::Enclosing(enclosing) => Some(Elem::Enclosing(without_crudkit_enclosing_field(
                enclosing, field_name,
            ))),
            element => Some(element),
        })
        .collect()
}

fn without_crudkit_enclosing_field<F: TypeErasedField>(
    enclosing: Enclosing<F>,
    field_name: &str,
) -> Enclosing<F> {
    match enclosing {
        Enclosing::None(mut group) => {
            group.children = without_crudkit_field(group.children, field_name);
            Enclosing::None(group)
        }
        Enclosing::Tabs(tabs) => Enclosing::Tabs(
            tabs.into_iter()
                .map(|mut tab| {
                    tab.group.children = without_crudkit_field(tab.group.children, field_name);
                    tab
                })
                .collect(),
        ),
        Enclosing::Card(mut group) => {
            group.children = without_crudkit_field(group.children, field_name);
            Enclosing::Card(group)
        }
    }
}

fn item_content(page: ItemPage) -> AnyView {
    let ItemPage {
        projects,
        active_project_names,
        project,
        item,
        comments,
        relationships,
        label_suggestions,
        work_item_states,
        automation_runs,
        api_base_url,
        codex_status,
    } = page;
    provide_work_item_states_context(work_item_states);
    let topbar = top_bar(
        projects,
        active_project_names,
        Some(project.clone()),
        ActivePage::Board,
        None,
        codex_status,
    );
    let board_href = format!("/?project={}", encode_path(&project));
    let comment_action = format!(
        "/projects/{}/items/{}/comments",
        encode_path(&project),
        item.id
    );
    let header_title = format!("#{} {}", item.id, item.title);
    let item_state_display = state_label(&item).to_owned();
    let item_project_id = item.project_id;
    let item_id = item.id;
    let (item_editor_context, set_item_editor_context) = signal(None::<CrudInstanceContext>);
    let navigate = use_navigate();
    let board_href_for_exit = board_href.clone();
    let exit_to_board = Callback::new(move |()| {
        navigate(&board_href_for_exit, NavigateOptions::default());
    });
    let exit_to_board_for_link = exit_to_board;
    let editor_default_create_state = Signal::derive(|| DEFAULT_STATE_LABEL.to_owned());
    let item_detail_navigation = CrudNavigationConfig {
        show_delete: true,
        ..CrudNavigationConfig::embedded_single_entity()
    };
    let mut item_detail_config = work_items_crudkit_config_for_view(
        api_base_url,
        item_project_id,
        SerializableCrudView::Edit(crudkit_i64_id(item_id)),
        item_detail_navigation,
        editor_default_create_state,
        None,
        Signal::derive(Vec::<ProjectLabelView>::new),
    );
    item_detail_config.elements = without_crudkit_field(item_detail_config.elements, "id");
    let item_editor = view! {
        <div class="crudkit-item-detail" data-crudkit-leptos="work-item-detail">
            <CrudInstance
                name="work-item-detail"
                config=item_detail_config
                on_exit=exit_to_board
                on_context_created=Callback::new(move |context| {
                    set_item_editor_context.set(Some(context));
                })
            />
        </div>
    };
    let comment_submit = background_form_submit(true);
    let claim = item
        .claimed_by
        .clone()
        .map(|agent| claim_badge(&project, agent, "Claimed", item.claimed_at.clone()));
    let finished = item.finished_at.clone().map(|finished_at| {
        view! { <span>"finished " {finished_at}</span> }
    });
    let automation_run_views = automation_runs_view(&project, automation_runs);
    let comment_views = comments
        .into_iter()
        .map(|comment| {
            let author = comment
                .author_name
                .unwrap_or_else(|| comment.author_type.as_storage().to_owned());
            let author = comment_author_view(&project, comment.author_type, author);
            view! {
                <article>
                    <strong>{author}</strong>
                    <span>{comment.created_at}</span>
                    <p>{comment.body}</p>
                </article>
            }
        })
        .collect::<Vec<_>>();
    let labels = item_labels_view(&project, &item, label_suggestions);
    let relationship_views = item_relationships_view(&project, &item, relationships);

    view! {
        <div>
            {topbar}
            <main class="page-shell item-page">
                <section class="item-header">
                    <button
                        type="button"
                        class="link-button item-board-link"
                        on:click=move |_| {
                            if let Some(context) = item_editor_context.get_untracked() {
                                context.request_leave();
                            } else {
                                exit_to_board_for_link.run(());
                            }
                        }
                    >
                        "Board"
                    </button>
                    <h1>{header_title}</h1>
                </section>
                <section class="item-meta">
                    <span>{item_state_display}</span>
                    <span>"v" {item.version}</span>
                    {claim}
                    {finished}
                </section>
                <section class="item-settings panel">
                    <h2>"Item details"</h2>
                    {item_editor}
                </section>
                {labels}
                {relationship_views}
                {automation_run_views}
                <section class="comments">
                    <h2>"Comments"</h2>
                    {comment_views}
                    <form method="post" action=comment_action on:submit=comment_submit>
                        <input name="author_name" placeholder="Your name"/>
                        <textarea name="body" placeholder="Comment" required></textarea>
                        <button>"Add comment"</button>
                    </form>
                </section>
            </main>
        </div>
    }
    .into_any()
}

fn comment_author_view(project: &str, author_type: AuthorType, author: String) -> AnyView {
    if author_type == AuthorType::Agent
        && let Some(run_id) = infer_patchbay_run_id(&author)
    {
        let href = format!(
            "/projects/{}/automation/runs/{}/log",
            encode_path(project),
            run_id
        );
        return view! {
            <a class="comment-author-link" href=href>{author}</a>
        }
        .into_any();
    }

    view! { {author} }.into_any()
}

fn infer_patchbay_run_id(agent_id: &str) -> Option<i64> {
    let id = agent_id.strip_prefix("patchbay-run-")?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let run_id = id.parse::<i64>().ok()?;
    (run_id > 0).then_some(run_id)
}

fn item_relationships_view(
    project: &str,
    item: &WorkItemView,
    relationships: Vec<WorkItemRelationshipListEntry>,
) -> AnyView {
    let add_action = format!(
        "/projects/{}/items/{}/relationships",
        encode_path(project),
        item.id
    );
    let add_submit = background_form_submit(true);
    let rows = relationships
        .into_iter()
        .map(|entry| item_relationship_row(project, item.id, entry))
        .collect::<Vec<_>>();
    let empty = rows.is_empty().then(|| {
        view! { <p class="muted">"No relationships"</p> }
    });

    view! {
        <section class="item-relationships panel">
            <h2>"Relationships"</h2>
            <div class="relationship-list">
                {empty}
                {rows}
            </div>
            <form class="relationship-add-form" method="post" action=add_action on:submit=add_submit>
                <input
                    type="number"
                    min="1"
                    name="target_work_item_id"
                    placeholder="target item id"
                    required
                />
                <input name="kind" placeholder="kind" required/>
                <button>"Add relationship"</button>
            </form>
        </section>
    }
    .into_any()
}

fn item_relationship_row(
    project: &str,
    item_id: i64,
    entry: WorkItemRelationshipListEntry,
) -> impl IntoView + 'static {
    let relationship = entry.relationship;
    let related = match entry.direction {
        WorkItemRelationshipDirection::Outgoing => relationship.target.clone(),
        WorkItemRelationshipDirection::Incoming => relationship.source.clone(),
    };
    let update_action = format!(
        "/projects/{}/items/{}/relationships/{}/update",
        encode_path(project),
        item_id,
        relationship.id
    );
    let delete_action = format!(
        "/projects/{}/items/{}/relationships/{}/delete",
        encode_path(project),
        item_id,
        relationship.id
    );
    let update_submit = background_form_submit(false);
    let delete_submit = background_form_submit(false);
    let direction = entry.direction.to_string();
    let related_href = item_href(project, related.id);
    let source_link = relationship_endpoint_link(project, &relationship.source);
    let target_link = relationship_endpoint_link(project, &relationship.target);
    let related_state = relationship_item_state_label(&related).to_owned();

    view! {
        <article class="relationship-row">
            <div class="relationship-main">
                <span class="relationship-direction">{direction}</span>
                <strong>{relationship.kind.clone()}</strong>
                <p>
                    {source_link}
                    <span class="relationship-kind">" -- " {relationship.kind.clone()} " --> "</span>
                    {target_link}
                </p>
                <a class="relationship-related" href=related_href>
                    "#"{related.id} " [" {related_state} "] " {related.title}
                </a>
            </div>
            <form method="post" action=update_action class="relationship-kind-form" on:submit=update_submit>
                <input name="kind" value=relationship.kind required/>
                <button>"Update"</button>
            </form>
            <form method="post" action=delete_action on:submit=delete_submit>
                <button class="danger">"Delete"</button>
            </form>
        </article>
    }
}

fn relationship_endpoint_link(
    project: &str,
    item: &WorkItemRelationshipItemSummary,
) -> impl IntoView + 'static {
    let href = item_href(project, item.id);
    let state = relationship_item_state_label(item).to_owned();
    let title = item.title.clone();
    let id = item.id;
    view! {
        <a href=href>"#"{id} " [" {state} "] " {title}</a>
    }
}

fn relationship_item_state_label(item: &WorkItemRelationshipItemSummary) -> &str {
    item.state.as_deref().unwrap_or("(no state)")
}

fn item_labels_view(
    project: &str,
    item: &WorkItemView,
    suggestions: Vec<ProjectLabelView>,
) -> AnyView {
    let add_action = format!(
        "/projects/{}/items/{}/labels",
        encode_path(project),
        item.id
    );
    let suggestion_options = label_suggestion_options(&suggestions);
    let state_options = use_context::<WorkItemStatesContext>()
        .map(|context| context.states)
        .expect("work item states context should be provided before rendering item labels");
    let state_suggestions = state_options.get_untracked();
    let state_suggestion_options = state_suggestion_options(&state_suggestions);
    let add_submit = background_form_submit(true);
    let rows = item
        .labels
        .iter()
        .cloned()
        .map(|label| item_label_row(project, item, label, state_options))
        .collect::<Vec<_>>();

    view! {
        <section class="item-labels panel">
            <h2>"Labels"</h2>
            <datalist id="label-key-suggestions">{suggestion_options}</datalist>
            <datalist id="state-value-suggestions">{state_suggestion_options}</datalist>
            <div class="label-list">{rows}</div>
            <form class="label-add-form" method="post" action=add_action on:submit=add_submit>
                <input type="hidden" name="version" value=item.version.to_string()/>
                <input
                    name="key"
                    list="label-key-suggestions"
                    placeholder="key"
                    required
                />
                <input
                    name="value"
                    list="state-value-suggestions"
                    placeholder="value"
                />
                <button>"Add label"</button>
            </form>
        </section>
    }
    .into_any()
}

fn item_label_row(
    project: &str,
    item: &WorkItemView,
    label: WorkItemLabelView,
    work_item_states: ReadSignal<Vec<WorkItemStateView>>,
) -> impl IntoView + 'static {
    let update_action = format!(
        "/projects/{}/items/{}/labels/{}/update",
        encode_path(project),
        item.id,
        label.id
    );
    let delete_action = format!(
        "/projects/{}/items/{}/labels/{}/delete",
        encode_path(project),
        item.id,
        label.id
    );
    let value = label.value.clone().unwrap_or_default();
    let rendered = format_label(&label.key, label.value.as_deref());
    let is_state = label.key == STATE_LABEL_KEY;
    let can_delete = label.key != STATE_LABEL_KEY;
    let blocked = label.key == AUTOMATION_BLOCKED_LABEL_KEY;
    let feedback_requested = label.key == FEEDBACK_REQUESTED_LABEL_KEY;
    let update_submit = background_form_submit(false);
    let delete_submit = background_form_submit(false);

    view! {
        <article class="label-row">
            <span
                class="label-chip"
                class:blocked=blocked
                class:feedback=feedback_requested
            >
                {rendered}
            </span>
            <form
                method="post"
                action=update_action
                class=if is_state { "state-label-form" } else { "" }
                on:submit=update_submit
            >
                <input type="hidden" name="version" value=item.version.to_string()/>
                {if is_state {
                    let value = value.clone();
                    view! {
                        <input type="hidden" name="key" value=STATE_LABEL_KEY/>
                        <select name="value" class="state-label-select" required>
                            {move || state_label_option_views(work_item_states.get(), value.clone())}
                        </select>
                    }
                    .into_any()
                } else {
                    view! {
                        <input name="key" value=label.key required/>
                        <input name="value" value=value/>
                    }
                    .into_any()
                }}
                <button>"Update"</button>
            </form>
            {can_delete.then(|| view! {
                <form method="post" action=delete_action on:submit=delete_submit>
                    <input type="hidden" name="version" value=item.version.to_string()/>
                    <button class="danger">"Delete"</button>
                </form>
            })}
        </article>
    }
}

fn state_label_option_views(states: Vec<WorkItemStateView>, current_value: String) -> Vec<AnyView> {
    let mut has_current = false;
    let mut options = Vec::new();
    for state in states {
        let selected = state.identifier == current_value;
        has_current |= selected;
        options.push(
            view! {
                <option value=state.identifier selected=selected>
                    {state.name}
                </option>
            }
            .into_any(),
        );
    }

    if !current_value.is_empty() && !has_current {
        let label = format!("{current_value} (unknown)");
        options.push(
            view! {
                <option value=current_value selected=true>{label}</option>
            }
            .into_any(),
        );
    }

    if options.is_empty() {
        options.push(
            view! {
                <option value="" selected=true>"No states available"</option>
            }
            .into_any(),
        );
    }

    options
}

fn label_suggestion_options(suggestions: &[ProjectLabelView]) -> Vec<impl IntoView> {
    let mut keys = suggestions
        .iter()
        .map(|label| label.key.clone())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .map(|key| view! { <option value=key></option> })
        .collect()
}

fn state_suggestion_options(states: &[WorkItemStateView]) -> Vec<impl IntoView> {
    states
        .iter()
        .map(|state| state.identifier.clone())
        .map(|value| view! { <option value=value></option> })
        .collect()
}

fn patchbay_labels_panel() -> impl IntoView {
    view! {
        <section class="patchbay-labels panel">
            <div class="panel-heading">
                <h2>"Patchbay labels"</h2>
            </div>
            <div class="system-label-grid">
                <article>
                    <code>{STATE_LABEL_KEY}</code>
                    <span>"Swim-lane state."</span>
                </article>
                <article>
                    <code>{CLAIMED_FROM_STATE_LABEL_KEY}</code>
                    <span>"Temporary claim origin."</span>
                </article>
                <article>
                    <code>{AUTOMATION_BLOCKED_LABEL_KEY}</code>
                    <span>"Excluded from automation pickup."</span>
                </article>
                <article>
                    <code>{FEEDBACK_REQUESTED_LABEL_KEY}</code>
                    <span>"Waiting for user feedback."</span>
                </article>
            </div>
        </section>
    }
}

fn automation_runs_view(project: &str, runs: Vec<AgentRunView>) -> AnyView {
    if runs.is_empty() {
        return ().into_any();
    }

    let run_items = runs
        .into_iter()
        .map(|run| {
            let href = format!(
                "/projects/{}/automation/runs/{}/log",
                encode_path(project),
                run.id
            );
            let tokens = run.token_usage.map(run_token_usage_label);
            view! {
                <li>
                    <a href=href>"#" {run.id}</a>
                    " · "
                    {run.status.to_string()}
                    " · "
                    {run.mutability.to_string()}
                    {tokens.map(|tokens| view! {
                        <>
                            " · "
                            {tokens}
                        </>
                    })}
                    " · "
                    {run.result_summary}
                </li>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <section class="item-automation">
            <h2>"Automation runs"</h2>
            <ul class="compact-list">{run_items}</ul>
        </section>
    }
    .into_any()
}

fn run_log_content(page: RunLogPage) -> AnyView {
    let RunLogPage {
        projects,
        active_project_names,
        project,
        run_log,
        workspace_editors,
        codex_status,
    } = page;
    let topbar = top_bar(
        projects,
        active_project_names,
        Some(project.clone()),
        ActivePage::Board,
        None,
        codex_status,
    );
    let board_href = format!("/?project={}", encode_path(&project));
    let title = format!("Run #{}", run_log.run.id);
    let summary = run_result_summary(&run_log.run);
    let origin = run_origin_label(&run_log.run);
    let work_item = run_work_item_link(&project, run_log.run.work_item_id);
    let command = recorded_field(&run_log.run.command);
    let run_href = format!(
        "/projects/{}/automation/runs/{}/log",
        encode_path(&project),
        run_log.run.id
    );
    let working_dir =
        run_workspace_actions(&project, &run_log.run, workspace_editors, run_href.clone());
    let status_class = run_status_class(run_log.run.status);
    let memory_event = run_log.memory_event.as_ref().map(memory_event_ref_label);
    let token_usage = run_token_usage_text(&run_log.run);
    let commit_outcome = run_commit_outcome_label(&run_log.run);
    let cancel_action = if run_log.active {
        let action = format!(
            "/projects/{}/automation/runs/{}/cancel",
            encode_path(&project),
            run_log.run.id
        );
        Some(view! {
            <form method="post" action=action>
                <input type="hidden" name="return_to" value=run_href/>
                <button type="submit" class="danger">"Cancel run"</button>
            </form>
        })
    } else {
        None
    };
    let pr_url = run_log.run.pr_url.clone().map(|pr_url| {
        let href = pr_url.clone();
        view! {
            <>
                <dt>"pull request"</dt>
                <dd><a href=href>{pr_url}</a></dd>
            </>
        }
    });
    let output = run_output_view(run_log.output.clone());
    let prompt = run_log
        .prompt
        .unwrap_or_else(|| "No prompt file has been written.".to_owned());

    view! {
        <div>
            {topbar}
            <main class="page-shell run-log">
                <section class="item-header">
                    <a href=board_href>"Board"</a>
                    <h1>{title.clone()}</h1>
                    <p>
                        {run_log.run.status.to_string()}
                        " · "
                        {summary.clone()}
                    </p>
                    <div class="run-log-actions">{cancel_action}</div>
                </section>
                <section>
                    <h2>"Run"</h2>
                    <dl>
                        {origin.map(|origin| view! {
                            <>
                                <dt>"source"</dt>
                                <dd>{origin}</dd>
                            </>
                        })}
                        {work_item.map(|work_item| view! {
                            <>
                                <dt>"item"</dt>
                                <dd>{work_item}</dd>
                            </>
                        })}
                        <dt>"result"</dt>
                        <dd class=format!("run-result-inline {status_class}")>{summary}</dd>
                        <dt>"mutability"</dt>
                        <dd>{run_log.run.mutability.to_string()}</dd>
                        <dt>"command"</dt>
                        <dd>{command}</dd>
                        <dt>"working dir"</dt>
                        <dd>{working_dir}</dd>
                        <dt>"cleanup"</dt>
                        <dd>{run_log.run.cleanup_status}</dd>
                        <dt>"tokens"</dt>
                        <dd>{token_usage}</dd>
                        <dt>"commit"</dt>
                        <dd>{commit_outcome}</dd>
                        {memory_event.map(|memory_event| view! {
                            <>
                                <dt>"memory"</dt>
                                <dd>{memory_event}</dd>
                            </>
                        })}
                        {pr_url}
                    </dl>
                </section>
                <section>
                    <h2>"Prompt"</h2>
                    <pre>{prompt}</pre>
                </section>
                <section>
                    <h2>"Output"</h2>
                    {output}
                </section>
            </main>
        </div>
    }
    .into_any()
}

fn api_docs_content(page: ApiDocsPage) -> AnyView {
    let topbar = top_bar(
        page.projects,
        page.active_project_names,
        page.selected_project,
        ActivePage::Api,
        None,
        page.codex_status,
    );
    let custom_endpoints = [
        "GET /api/projects/{project}/memory",
        "PUT /api/projects/{project}/memory",
        "POST /api/projects/{project}/memory/append",
        "GET /api/projects/{project}/memory/events",
        "POST /api/projects/{project}/memory/events/compact",
        "GET /api/events/ws",
        "GET /api/projects/{project}/events",
        "GET /api/projects/{project}/items/{item_id}/events",
        "GET /api/projects/{project}/items/{item_id}/relationships",
        "POST /api/projects/{project}/items/{item_id}/relationships",
        "PATCH /api/projects/{project}/relationships/{relationship_id}",
        "DELETE /api/projects/{project}/relationships/{relationship_id}",
        "GET /api/projects/{project}/automation/sessions",
        "POST /projects/{project}/automation/start",
        "POST /projects/{project}/automation/stop",
        "POST /projects/{project}/automation/recover-stale-claims",
        "POST /projects/{project}/automation/cleanup-worktrees",
        "POST /projects/{project}/workspace/open",
        "POST /projects/{project}/automation/runs/{run_id}/workspace/open",
        "POST /projects/{project}/automation/runs/{run_id}/cancel",
        "POST /api/projects/{project}/items/{item_id}/request-feedback",
        "POST /system/database/open",
        "GET /projects/{project}/automation/runs/{run_id}/log",
    ]
    .into_iter()
    .map(|endpoint| view! { <li>{endpoint}</li> })
    .collect::<Vec<_>>();

    view! {
        <div>
            {topbar}
            <main class="page-shell api-docs">
                <section class="page-heading">
                    <h1>"Patchbay API"</h1>
                </section>
                {patchbay_labels_panel()}
                <section class="panel">
                    <h2>"Custom endpoints"</h2>
                    <ul class="compact-list">{custom_endpoints}</ul>
                </section>
            </main>
        </div>
    }
    .into_any()
}

fn error_content(message: String) -> AnyView {
    view! {
        <main class="error">
            <h1>"Error"</h1>
            <p>{message}</p>
            <a href="/">"Back"</a>
        </main>
    }
    .into_any()
}

fn project_workspace_panel(
    project: &str,
    project_view: &ProjectView,
    workspace_editors: Vec<WorkspaceEditorView>,
    return_to: String,
) -> AnyView {
    workspace_actions(
        "Path",
        project_view.path.clone(),
        Some(project_view.path_exists),
        project_view.git_status.clone(),
        Some(format!("/projects/{}/workspace/open", encode_path(project))),
        workspace_editors,
        return_to,
    )
}

fn run_workspace_actions(
    project: &str,
    run: &AgentRunView,
    workspace_editors: Vec<WorkspaceEditorView>,
    return_to: String,
) -> AnyView {
    workspace_actions(
        "working dir",
        non_empty_string(run.working_dir.clone()),
        None,
        None,
        Some(format!(
            "/projects/{}/automation/runs/{}/workspace/open",
            encode_path(project),
            run.id
        )),
        workspace_editors,
        return_to,
    )
}

fn workspace_actions(
    label: &'static str,
    path: Option<String>,
    path_exists: Option<bool>,
    git_status: Option<ProjectGitStatusView>,
    open_action: Option<String>,
    workspace_editors: Vec<WorkspaceEditorView>,
    return_to: String,
) -> AnyView {
    let path = path.and_then(non_empty_string);
    let copy_available = path.is_some();
    let open_available = copy_available && path_exists.unwrap_or(true);
    let display_path = path.clone().unwrap_or_else(|| "not configured".to_owned());
    let copy_path = path.clone().unwrap_or_default();
    let (copy_message, set_copy_message) = signal(None::<String>);
    let status = path_exists.map(|exists| {
        view! {
            <span class=if exists {
                "workspace-status workspace-status-ok"
            } else {
                "workspace-status workspace-status-missing"
            }>
                {if exists { "Exists" } else { "Missing" }}
            </span>
        }
    });
    let git_status = git_status.map(workspace_git_status);
    let open_controls = open_action.map(|action| {
        let folder_action = action.clone();
        let folder_return = return_to.clone();
        let editor_controls = workspace_editors
            .into_iter()
            .map(|editor| {
                let editor_action = action.clone();
                let editor_return = return_to.clone();
                let target = editor.target.clone();
                let label = format!("Open {}", editor.label);
                let icon_src = workspace_editor_icon_src(&editor.target);
                view! {
                    <form method="post" action=editor_action>
                        <input type="hidden" name="target" value=target/>
                        <input type="hidden" name="return_to" value=editor_return/>
                        <button type="submit" class="secondary workspace-button" disabled=!open_available>
                            {icon_src.map(|src| view! {
                                <img class="workspace-button-icon" src=src alt="" aria-hidden="true"/>
                            })}
                            <span>{label}</span>
                        </button>
                    </form>
                }
            })
            .collect::<Vec<_>>();
        view! {
            <>
                <form method="post" action=folder_action>
                    <input type="hidden" name="target" value="folder"/>
                    <input type="hidden" name="return_to" value=folder_return/>
                    <button type="submit" class="secondary workspace-button" disabled=!open_available>
                        "Open folder"
                    </button>
                </form>
                {editor_controls}
            </>
        }
    });
    let path_for_copy = copy_path.clone();

    view! {
        <div class="workspace-actions">
            <div class="workspace-path">
                <span class="workspace-label">{label}</span>
                <code>{display_path}</code>
                {status}
            </div>
            {git_status}
            <div class="workspace-buttons">
                <button
                    type="button"
                    class="secondary workspace-button"
                    disabled=!copy_available
                    on:click=move |_| {
                        copy_workspace_text(
                            path_for_copy.clone(),
                            "Copied path",
                            set_copy_message,
                        );
                    }
                >
                    "Copy path"
                </button>
                {open_controls}
                {move || {
                    copy_message
                        .get()
                        .map(|message| view! { <span class="workspace-copy-status">{message}</span> })
                }}
            </div>
        </div>
    }
    .into_any()
}

fn workspace_git_status(status: ProjectGitStatusView) -> AnyView {
    if !status.is_repository {
        let message = match status.error {
            Some(error) => view! {
                <span class="workspace-status workspace-status-missing" title=error>
                    "Git unavailable"
                </span>
            }
            .into_any(),
            None => view! {
                <span class="workspace-status workspace-status-neutral">
                    "Not a Git repository"
                </span>
            }
            .into_any(),
        };
        return view! { <div class="workspace-git-status">{message}</div> }.into_any();
    }

    let branch = status.branch.unwrap_or_else(|| "unknown branch".to_owned());
    let additions = format!("+{}", status.added_lines);
    let deletions = format!("-{}", status.deleted_lines);
    let diff_status = status.error.map(|error| {
        view! {
            <span class="workspace-status workspace-status-missing" title=error>
                "Diff unavailable"
            </span>
        }
    });

    view! {
        <div class="workspace-git-status">
            <span class="workspace-status workspace-status-ok">"Git repository"</span>
            <span class="workspace-git-branch">{branch}</span>
            <span class="workspace-git-diff" aria-label="Git line diff">
                <span class="workspace-git-added">{additions}</span>
                <span class="workspace-git-deleted">{deletions}</span>
            </span>
            {diff_status}
        </div>
    }
    .into_any()
}

fn workspace_editor_icon_src(target: &str) -> Option<&'static str> {
    match target {
        "rustrover" => Some("/icons/workspace-rustrover.svg"),
        "vscode" => Some("/icons/workspace-vscode.svg"),
        _ => None,
    }
}

fn runtime_panel(runtime: RuntimeConfigView, return_to: String) -> AnyView {
    view! {
        <section class="runtime-panel panel">
            <div class="panel-heading">
                <h2>"Runtime"</h2>
            </div>
            <div class="runtime-paths">
                {database_path_actions(&runtime, return_to)}
                {readonly_path_row("Database directory", runtime.database_directory)}
                {readonly_path_row("Codex home", runtime.codex_home_path)}
                {readonly_path_row("Codex config", runtime.codex_config_path)}
            </div>
        </section>
    }
    .into_any()
}

fn database_path_actions(runtime: &RuntimeConfigView, return_to: String) -> AnyView {
    let database_path = runtime.database_path.clone();
    let path_for_copy = database_path.clone();
    let (copy_message, set_copy_message) = signal(None::<String>);

    view! {
        <div class="workspace-actions">
            <div class="workspace-path">
                <span class="workspace-label">"Database file"</span>
                <code>{database_path}</code>
                <span class="workspace-status workspace-status-ok">"Active"</span>
            </div>
            <div class="workspace-buttons">
                <button
                    type="button"
                    class="secondary workspace-button"
                    on:click=move |_| {
                        copy_workspace_text(
                            path_for_copy.clone(),
                            "Copied path",
                            set_copy_message,
                        );
                    }
                >
                    "Copy path"
                </button>
                <form method="post" action="/system/database/open">
                    <input type="hidden" name="return_to" value=return_to/>
                    <button type="submit" class="secondary workspace-button">
                        "Open directory"
                    </button>
                </form>
                {move || {
                    copy_message
                        .get()
                        .map(|message| view! { <span class="workspace-copy-status">{message}</span> })
                }}
            </div>
        </div>
    }
    .into_any()
}

fn readonly_path_row(label: &'static str, path: String) -> AnyView {
    view! {
        <div class="workspace-actions">
            <div class="workspace-path">
                <span class="workspace-label">{label}</span>
                <code>{path}</code>
            </div>
        </div>
    }
    .into_any()
}

fn non_empty_string(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn copy_workspace_text(
    text: String,
    success_message: &'static str,
    set_copy_message: WriteSignal<Option<String>>,
) {
    leptos::task::spawn_local(async move {
        let message = match write_clipboard_text(text).await {
            Ok(()) => success_message.to_owned(),
            Err(err) => err,
        };
        set_copy_message.set(Some(message));
    });
}

#[cfg(not(feature = "ssr"))]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export async function patchbayCopyText(text) {
  if (navigator.clipboard && window.isSecureContext) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.left = '-9999px';
  textarea.style.top = '0';
  document.body.appendChild(textarea);
  textarea.focus();
  textarea.select();
  const copied = document.execCommand('copy');
  textarea.remove();
  if (!copied) {
    throw new Error('Copy failed');
  }
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(catch, js_name = patchbayCopyText)]
    async fn js_copy_text(text: &str) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>;
}

#[cfg(not(feature = "ssr"))]
async fn write_clipboard_text(text: String) -> Result<(), String> {
    js_copy_text(&text)
        .await
        .map(|_| ())
        .map_err(js_error_message)
}

#[cfg(feature = "ssr")]
async fn write_clipboard_text(_text: String) -> Result<(), String> {
    Ok(())
}

#[cfg(not(feature = "ssr"))]
fn js_error_message(value: wasm_bindgen::JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| "Copy failed".to_owned())
}

#[component]
fn LiveBoardItems(
    project: String,
    initial_items: Vec<WorkItemView>,
    initial_swim_lanes: Vec<SwimLaneView>,
    initial_misconfigured_item_count: i64,
    open_create_item: Callback<CreateItemOpenRequest>,
) -> impl IntoView + 'static {
    let (items, set_items) = signal(initial_items);
    let (swim_lanes, set_swim_lanes) = signal(initial_swim_lanes);
    let work_item_states_context = use_context::<WorkItemStatesContext>()
        .expect("work item states context should be provided before rendering board items");
    let work_item_states = work_item_states_context.states;
    let set_work_item_states = work_item_states_context.set_states;
    let (misconfigured_item_count, set_misconfigured_item_count) =
        signal(initial_misconfigured_item_count);
    let project_for_loader = project.clone();
    let section = LocalResource::new(move || load_board_items_section(project_for_loader.clone()));
    notify_resource_errors(section, || true);
    let _poll = use_interval_fn(move || section.refetch(), BOARD_ITEMS_REFRESH_INTERVAL_MS);
    let project_for_events = project.clone();
    refetch_on_live_event(section, move |event| {
        event_scopes_named_project(event, Some(project_for_events.as_str()))
            && matches!(
                event,
                UiEvent::WorkItemChanged { .. }
                    | UiEvent::AgentRunChanged { .. }
                    | UiEvent::SwimLaneChanged { .. }
                    | UiEvent::WorkItemStateChanged { .. }
            )
    });

    Effect::new(move |_| {
        if let Some(Ok(section)) = section.get() {
            set_items.set(section.items);
            let updated_swim_lanes = section.swim_lanes;
            let updated_work_item_states = section.work_item_states;
            set_swim_lanes.set(updated_swim_lanes);
            set_work_item_states.set(updated_work_item_states);
            set_misconfigured_item_count.set(section.misconfigured_item_count);
        }
    });

    view! {
        {move || {
            board_view(
                project.clone(),
                items.get(),
                swim_lanes.get(),
                work_item_states.get(),
                misconfigured_item_count.get(),
                open_create_item,
            )
        }}
    }
}

#[component]
fn LiveRunsSection(
    project: String,
    initial_status: AutomationStatusView,
    initial_running: bool,
    initial_run_sessions: Vec<BoardRunSessionView>,
    workspace_editors: Vec<WorkspaceEditorView>,
) -> impl IntoView + 'static {
    let (automation_status, set_automation_status) = signal(initial_status);
    let (automation_running, set_automation_running) = signal(initial_running);
    let (run_sessions, set_run_sessions) = signal(initial_run_sessions);
    let project_for_loader = project.clone();
    let section = LocalResource::new(move || load_runs_section(project_for_loader.clone()));
    notify_resource_errors(section, || true);
    let project_for_events = project.clone();
    refetch_on_live_event(section, move |event| {
        event_scopes_named_project(event, Some(project_for_events.as_str()))
            && matches!(
                event,
                UiEvent::AutomationChanged { .. }
                    | UiEvent::AgentRunChanged { .. }
                    | UiEvent::AgentOutputChanged { .. }
                    | UiEvent::CodexStatusChanged { .. }
            )
    });

    Effect::new(move |_| {
        if let Some(Ok(section)) = section.get() {
            set_automation_status.set(section.automation_status);
            set_automation_running.set(section.automation_running);
            set_run_sessions.set(section.run_sessions);
        }
    });

    let status_note = Signal::derive(move || {
        let status = automation_status.get();
        let running_runs = status.running_runs;
        let mutating = status.running_mutating_runs;
        let read_only = status.running_read_only_runs;
        let controller = if automation_running.get() {
            "controller running"
        } else {
            "controller stopped"
        };
        Some(format!(
            "{running_runs} running ({mutating} mutating, {read_only} read-only), {controller}"
        ))
    });

    view! {
        <RunSessionsPanel
            project=project
            title="Runs"
            status_note=status_note
            run_sessions=run_sessions
            workspace_editors=workspace_editors
            sync_selection_with_url=true
            empty_message="No runs yet."
        />
    }
}

fn project_settings_view(
    project: &str,
    project_view: ProjectView,
    settings: ProjectSettingsView,
    system_prompt_events: Vec<ProjectSystemPromptEventView>,
    memory_events: Vec<ProjectMemoryEventView>,
    auto_commit: ReadSignal<bool>,
    set_auto_commit: WriteSignal<bool>,
) -> impl IntoView + 'static {
    let prompt_action = format!("/projects/{}/system-prompt", encode_path(project));
    let memory_action = format!("/projects/{}/memory", encode_path(project));
    let commit_policy_action = format!("/projects/{}/settings/commit-policy", encode_path(project));
    let commit_standard = settings.commit_standard.clone();
    let max_read_only_agents = settings.max_read_only_agents.to_string();
    let manual_revert_selected = settings.revert_strategy == RevertStrategy::Manual;
    let git_reset_selected = settings.revert_strategy == RevertStrategy::GitReset;
    let git_policy = settings.agent_git_command_policy.clone();
    let hard_reset_never_selected = git_policy.hard_reset == AgentGitHardResetPolicy::Never;
    let hard_reset_isolated_selected =
        git_policy.hard_reset == AgentGitHardResetPolicy::IsolatedWorkspaces;
    let initial_system_prompt = project_view.system_prompt.clone();
    let system_prompt_dirty_baseline = initial_system_prompt.clone();
    let system_prompt_history_for_options = system_prompt_events.clone();
    let system_prompt_history_for_prompt = system_prompt_events;
    let (selected_system_prompt_event_id, set_selected_system_prompt_event_id) =
        signal(None::<i64>);
    let (system_prompt_draft, set_system_prompt_draft) = signal(initial_system_prompt.clone());
    let system_prompt_value = move || {
        selected_system_prompt_event_id
            .get()
            .and_then(|event_id| {
                system_prompt_history_for_prompt
                    .iter()
                    .find(|event| event.id == event_id)
                    .map(|event| event.system_prompt.clone())
                    .or_else(|| {
                        Some(format!(
                            "System prompt event #{event_id} is no longer available."
                        ))
                    })
            })
            .unwrap_or_else(|| system_prompt_draft.get())
    };
    let system_prompt_textarea_class = move || {
        if selected_system_prompt_event_id.get().is_none()
            && system_prompt_draft.get() != system_prompt_dirty_baseline
        {
            "project-system-prompt-text dirty"
        } else {
            "project-system-prompt-text"
        }
    };
    let system_prompt_event_options = system_prompt_history_for_options
        .into_iter()
        .map(|event| {
            view! {
                <option value=event.id.to_string()>{system_prompt_event_select_label(&event)}</option>
            }
        })
        .collect::<Vec<_>>();
    let initial_memory = project_view.memory.clone();
    let memory_dirty_baseline = initial_memory.clone();
    let memory_history_for_options = memory_events.clone();
    let memory_history_for_memory = memory_events.clone();
    let (selected_memory_event_id, set_selected_memory_event_id) = signal(None::<i64>);
    let (memory_draft, set_memory_draft) = signal(initial_memory.clone());
    let memory_value = move || {
        selected_memory_event_id
            .get()
            .and_then(|event_id| {
                memory_history_for_memory
                    .iter()
                    .find(|event| event.id == event_id)
                    .map(|event| event.memory.clone())
                    .or_else(|| Some(format!("Memory event #{event_id} is no longer available.")))
            })
            .unwrap_or_else(|| memory_draft.get())
    };
    let memory_textarea_class = move || {
        if selected_memory_event_id.get().is_none() && memory_draft.get() != memory_dirty_baseline {
            "project-memory-text dirty"
        } else {
            "project-memory-text"
        }
    };
    let memory_event_options = memory_history_for_options
        .into_iter()
        .map(|event| {
            view! {
                <option value=event.id.to_string()>{memory_event_select_label(&event)}</option>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <section class="project-settings">
            <div>
                <h2>"System prompt"</h2>
                <form method="post" action=prompt_action>
                    <div class="project-text-history">
                        <label for="project-system-prompt-version">"system prompt history"</label>
                        <select
                            id="project-system-prompt-version"
                            prop:value=move || {
                                selected_system_prompt_event_id
                                    .get()
                                    .map(|event_id| event_id.to_string())
                                    .unwrap_or_else(|| "current".to_owned())
                            }
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if selected == "current" {
                                    set_selected_system_prompt_event_id.set(None);
                                } else if let Ok(event_id) = selected.parse::<i64>() {
                                    set_selected_system_prompt_event_id.set(Some(event_id));
                                }
                            }
                        >
                            <option value="current">"Current"</option>
                            {system_prompt_event_options}
                        </select>
                    </div>
                    <textarea
                        name="body"
                        class=system_prompt_textarea_class
                        placeholder="Project system prompt"
                        prop:value=system_prompt_value
                        readonly=move || selected_system_prompt_event_id.get().is_some()
                        on:input=move |event| {
                            if selected_system_prompt_event_id.get().is_none() {
                                set_system_prompt_draft.set(event_target_value(&event));
                            }
                        }
                    >
                        {initial_system_prompt}
                    </textarea>
                    <button disabled=move || selected_system_prompt_event_id.get().is_some()>
                        "Save prompt"
                    </button>
                </form>
            </div>
            <div>
                <h2>"Memory"</h2>
                <form method="post" action=memory_action>
                    <div class="project-text-history">
                        <label for="project-memory-version">"memory history"</label>
                        <select
                            id="project-memory-version"
                            prop:value=move || {
                                selected_memory_event_id
                                    .get()
                                    .map(|event_id| event_id.to_string())
                                    .unwrap_or_else(|| "current".to_owned())
                            }
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if selected == "current" {
                                    set_selected_memory_event_id.set(None);
                                } else if let Ok(event_id) = selected.parse::<i64>() {
                                    set_selected_memory_event_id.set(Some(event_id));
                                }
                            }
                        >
                            <option value="current">"Current"</option>
                            {memory_event_options}
                        </select>
                    </div>
                    <textarea
                        name="body"
                        class=memory_textarea_class
                        placeholder="Project memory"
                        prop:value=memory_value
                        readonly=move || selected_memory_event_id.get().is_some()
                        on:input=move |event| {
                            if selected_memory_event_id.get().is_none() {
                                set_memory_draft.set(event_target_value(&event));
                            }
                        }
                    >
                        {initial_memory}
                    </textarea>
                    <button disabled=move || selected_memory_event_id.get().is_some()>
                        "Save memory"
                    </button>
                </form>
            </div>
            <div class="commit-policy">
                <h2>"Automation policy"</h2>
                <form method="post" action=commit_policy_action>
                    <label for="project-max-read-only-agents">"Read-only agents"</label>
                    <input
                        id="project-max-read-only-agents"
                        type="number"
                        min="0"
                        step="1"
                        name="max_read_only_agents"
                        value=max_read_only_agents
                    />
                    <label class="checkbox-row" for="project-auto-commit">
                        <input
                            id="project-auto-commit"
                            type="checkbox"
                            name="auto_commit"
                            prop:checked=move || auto_commit.get()
                            on:change=move |event| {
                                set_auto_commit.set(event_target_checked(&event));
                            }
                        />
                        <span>"Auto-Commit"</span>
                    </label>
                    <label for="project-commit-standard">"Commit standard"</label>
                    <textarea
                        id="project-commit-standard"
                        name="commit_standard"
                        placeholder="Commit message standard"
                    >
                        {commit_standard}
                    </textarea>
                    <label for="project-revert-strategy">"Failure revert"</label>
                    <select id="project-revert-strategy" name="revert_strategy">
                        <option value="manual" selected=manual_revert_selected>"revert manually"</option>
                        <option value="git_reset" selected=git_reset_selected>"git reset"</option>
                    </select>
                    <div class="git-command-policy">
                        <label class="checkbox-row" for="project-git-add">
                            <input
                                id="project-git-add"
                                type="checkbox"
                                name="git_add"
                                checked=git_policy.add
                            />
                            <span>"git add"</span>
                        </label>
                        <label class="checkbox-row" for="project-git-commit">
                            <input
                                id="project-git-commit"
                                type="checkbox"
                                name="git_commit"
                                checked=git_policy.commit
                            />
                            <span>"git commit"</span>
                        </label>
                        <label class="checkbox-row" for="project-git-push">
                            <input
                                id="project-git-push"
                                type="checkbox"
                                name="git_push"
                                checked=git_policy.push
                            />
                            <span>"git push"</span>
                        </label>
                        <label class="checkbox-row" for="project-git-reset">
                            <input
                                id="project-git-reset"
                                type="checkbox"
                                name="git_reset"
                                checked=git_policy.reset
                            />
                            <span>"git reset"</span>
                        </label>
                    </div>
                    <label for="project-git-hard-reset">"Hard reset"</label>
                    <select id="project-git-hard-reset" name="git_hard_reset">
                        <option value="isolated_workspaces" selected=hard_reset_isolated_selected>
                            "isolated branches/worktrees only"
                        </option>
                        <option value="never" selected=hard_reset_never_selected>"never"</option>
                    </select>
                    <button>"Save policy"</button>
                </form>
            </div>
        </section>
    }
}

fn memory_event_select_label(event: &ProjectMemoryEventView) -> String {
    format!("#{} {}", event.id, event.created_at)
}

fn system_prompt_event_select_label(event: &ProjectSystemPromptEventView) -> String {
    format!("#{} {}", event.id, event.created_at)
}

fn memory_event_ref_label(event: &ProjectMemoryEventRefView) -> String {
    if event.available {
        match event.created_at.as_deref() {
            Some(created_at) => format!("MemoryChanged #{} {}", event.event_id, created_at),
            None => format!("MemoryChanged #{}", event.event_id),
        }
    } else {
        format!("MemoryChanged #{} unavailable", event.event_id)
    }
}

fn trigger_runs_panel(
    project: String,
    selected_trigger_id: Memo<Option<i64>>,
    workspace_editors: Vec<WorkspaceEditorView>,
) -> impl IntoView + 'static {
    let project_for_loader = project.clone();
    let project_for_view = project.clone();
    let trigger_runs = LocalResource::new(move || {
        let project = project_for_loader.clone();
        let trigger_id = selected_trigger_id.get();
        async move {
            match trigger_id {
                Some(trigger_id) => load_trigger_run_sessions(project, trigger_id)
                    .await
                    .map(Some),
                None => Ok(None),
            }
        }
    });
    let project_for_events = project.clone();
    refetch_on_live_event(trigger_runs, move |event| {
        event_scopes_named_project(event, Some(project_for_events.as_str()))
            && matches!(
                event,
                UiEvent::AutomationChanged { .. }
                    | UiEvent::AgentRunChanged { .. }
                    | UiEvent::AgentOutputChanged { .. }
                    | UiEvent::CodexStatusChanged { .. }
            )
            && selected_trigger_id.get().is_some()
    });
    let (run_sessions, set_run_sessions) = signal(Vec::<BoardRunSessionView>::new());
    let (load_error, set_load_error) = signal(None::<String>);
    Effect::new(move |_| {
        if let Some(result) = trigger_runs.get() {
            match result {
                Ok(Some(sessions)) => {
                    set_load_error.set(None);
                    set_run_sessions.set(sessions);
                }
                Ok(None) => {
                    set_load_error.set(None);
                    set_run_sessions.set(Vec::new());
                }
                Err(err) => {
                    set_load_error.set(Some(err.to_string()));
                }
            }
        }
    });

    view! {
        {move || {
            if selected_trigger_id.get().is_some() {
                let schedule_action = selected_trigger_id.get().map(|trigger_id| {
                    format!(
                        "/projects/{}/automation/triggers/{}/schedule-evaluation",
                        encode_path(&project_for_view),
                        trigger_id
                    )
                });
                let error = load_error.get();
                view! {
                    {schedule_action.map(|action| {
                        view! {
                            <section class="automation trigger-actions panel">
                                <div class="panel-heading">
                                    <h2>"Selected automation"</h2>
                                </div>
                                <form method="post" action=action>
                                    <button type="submit">"Queue evaluation"</button>
                                </form>
                            </section>
                        }
                    })}
                    {if let Some(error) = error {
                        view! {
                            <section class="automation trigger-runs">
                                <div class="panel-heading">
                                    <h2>"Runs"</h2>
                                </div>
                                <p class="system-alert">{error}</p>
                            </section>
                        }.into_any()
                    } else {
                        view! {
                            <RunSessionsPanel
                                project=project_for_view.clone()
                                title="Runs for selected automation"
                                status_note=Signal::derive(|| None::<String>)
                                run_sessions=run_sessions
                                workspace_editors=workspace_editors.clone()
                                sync_selection_with_url=false
                                empty_message="No runs for this automation yet."
                            />
                        }.into_any()
                    }}
                }.into_any()
            } else {
                view! {
                <section class="automation trigger-runs">
                    <div class="panel-heading">
                        <h2>"Runs"</h2>
                        <p class="muted">"Edit or inspect an automation to filter this panel."</p>
                    </div>
                    <p class="muted">"No automation selected."</p>
                </section>
                }.into_any()
            }
        }}
    }
}

#[component]
fn RunSessionsPanel(
    project: String,
    title: &'static str,
    #[prop(into)] status_note: Signal<Option<String>>,
    #[prop(into)] run_sessions: ReadSignal<Vec<BoardRunSessionView>>,
    workspace_editors: Vec<WorkspaceEditorView>,
    sync_selection_with_url: bool,
    empty_message: &'static str,
) -> impl IntoView + 'static {
    let query = use_query_map();
    let initial_selected_run_id = if sync_selection_with_url {
        query
            .read_untracked()
            .get("run")
            .and_then(|value| value.parse::<i64>().ok())
    } else {
        None
    };
    let (selected_run_id, set_selected_run_id) = signal(initial_selected_run_id);
    Effect::new(move |_| {
        if !sync_selection_with_url {
            return;
        }
        let query_selected = query
            .read()
            .get("run")
            .and_then(|value| value.parse::<i64>().ok());
        if let Some(run_id) = query_selected
            && selected_run_id.get_untracked() != Some(run_id)
        {
            set_selected_run_id.set(Some(run_id));
        }
    });
    Effect::new(move |_| {
        let sessions = run_sessions.get();
        let selected = selected_run_id.get_untracked();
        let selected_still_exists = selected
            .map(|run_id| sessions.iter().any(|session| session.run.id == run_id))
            .unwrap_or(false);
        let next = if sessions.is_empty() {
            None
        } else if selected_still_exists {
            selected
        } else {
            sessions.first().map(|session| session.run.id)
        };
        if selected != next {
            set_selected_run_id.set(next);
        }
    });

    let navigate = use_navigate();
    let selection_project = project.clone();
    let select_run = Callback::new(move |run_id: i64| {
        set_selected_run_id.set(Some(run_id));
        if sync_selection_with_url {
            let href = format!(
                "/runs?project={}&run={run_id}",
                encode_path(&selection_project)
            );
            navigate(
                &href,
                NavigateOptions {
                    replace: true,
                    scroll: false,
                    ..NavigateOptions::default()
                },
            );
        }
    });

    let run_items = move || {
        let sessions = run_sessions.get();
        if sessions.is_empty() {
            return view! { <p class="muted">{empty_message}</p> }.into_any();
        }
        let sessions = sessions
            .into_iter()
            .map(|session| {
                let run_id = session.run.id;
                let is_active = session.active;
                let summary = run_result_summary(&session.run);
                let origin = run_origin_label(&session.run);
                let item = run_item_label(&session.run);
                let tokens = session.run.token_usage.map(run_token_usage_label);
                let status_class = run_status_class(session.run.status);
                let selected_signal = selected_run_id;
                view! {
                    <button
                        type="button"
                        class=move || {
                            let selected = if selected_signal.get() == Some(run_id) {
                                " selected"
                            } else {
                                ""
                            };
                            format!("run-session {status_class}{selected}")
                        }
                        aria-pressed=move || selected_signal.get() == Some(run_id)
                        on:click=move |_| select_run.run(run_id)
                    >
                        <div class="session-head">
                            <strong>"#" {run_id}</strong>
                            <span>{session.run.status.to_string()}</span>
                            {item.map(|item| view! { <span>{item}</span> })}
                            {origin.map(|origin| view! { <span>{origin}</span> })}
                            {tokens.map(|tokens| view! { <span>{tokens}</span> })}
                            {is_active.then(|| view! { <span class="live-badge">"active"</span> })}
                        </div>
                        <p>{summary}</p>
                    </button>
                }
            })
            .collect::<Vec<_>>();
        view! { <div class="run-session-list">{sessions}</div> }.into_any()
    };
    let detail_project = project.clone();
    let detail_workspace_editors = workspace_editors.clone();
    let run_detail = move || {
        let detail_sessions = run_sessions.get();
        let selected = selected_run_id
            .get()
            .and_then(|run_id| {
                detail_sessions
                    .iter()
                    .find(|session| session.run.id == run_id)
                    .cloned()
            })
            .or_else(|| detail_sessions.first().cloned());
        match selected {
            Some(session) => {
                run_session_detail(&detail_project, session, detail_workspace_editors.clone())
            }
            None => view! { <p class="muted">"No run selected."</p> }.into_any(),
        }
    };

    view! {
        <section class="automation">
            <div class="panel-heading">
                <h2>{title}</h2>
                {move || status_note.get().map(|note| view! { <p class="muted">{note}</p> })}
            </div>
            <div class="run-session-shell">
                {run_items}
                <aside class="run-session-detail">
                    {run_detail}
                </aside>
            </div>
        </section>
    }
}

fn run_status_class(status: AgentRunStatus) -> String {
    format!("status-{}", status.as_storage())
}

fn run_result_summary(run: &AgentRunView) -> String {
    if run.result_summary.trim().is_empty() {
        "No summary yet.".to_owned()
    } else {
        run.result_summary.clone()
    }
}

fn run_commit_outcome_label(run: &AgentRunView) -> String {
    let requirement = if run.commit_required {
        "required"
    } else {
        "not required"
    };
    let base = match run.commit_outcome {
        AgentCommitOutcome::NotEvaluated => "not evaluated".to_owned(),
        AgentCommitOutcome::NotRequired => "not required by policy".to_owned(),
        AgentCommitOutcome::Committed => {
            if run.commit_shas.is_empty() {
                "committed".to_owned()
            } else {
                let shas = run
                    .commit_shas
                    .iter()
                    .map(|sha| sha.chars().take(12).collect::<String>())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("committed {shas}")
            }
        }
        AgentCommitOutcome::SkippedNoChanges => "skipped: no changes".to_owned(),
        AgentCommitOutcome::SkippedNoGitRepo => "skipped: no git repository".to_owned(),
        AgentCommitOutcome::MissingRequired => "missing required commit".to_owned(),
        AgentCommitOutcome::Unknown => "unknown".to_owned(),
    };
    format!("{base} ({requirement})")
}

fn run_token_usage_text(run: &AgentRunView) -> String {
    run.token_usage
        .map(run_token_usage_label)
        .unwrap_or_else(|| "not reported".to_owned())
}

fn run_token_usage_label(usage: AgentRunTokenUsageView) -> String {
    format!(
        "{} total ({} input, {} cached input, {} output)",
        format_number(usage.total_tokens),
        format_number(usage.input_tokens),
        format_number(usage.cached_input_tokens),
        format_number(usage.output_tokens)
    )
}

fn run_origin_label(run: &AgentRunView) -> Option<String> {
    run.trigger_id.map(|trigger_id| {
        let trigger_name = run
            .trigger_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        match trigger_name {
            Some(trigger_name) => format!("trigger #{trigger_id} {trigger_name}"),
            None => format!("trigger #{trigger_id}"),
        }
    })
}

fn run_item_label(run: &AgentRunView) -> Option<String> {
    run.work_item_id.map(|item_id| format!("item #{item_id}"))
}

fn run_work_item_link(project: &str, item_id: Option<i64>) -> Option<AnyView> {
    item_id.map(|item_id| {
        let href = format!("/projects/{}/items/{}", encode_path(project), item_id);
        view! {
            <a class="run-item-link" href=href>"Item #" {item_id}</a>
        }
        .into_any()
    })
}

fn recorded_field(value: &str) -> String {
    if value.trim().is_empty() {
        "not recorded".to_owned()
    } else {
        value.to_owned()
    }
}

fn run_session_detail(
    project: &str,
    session: BoardRunSessionView,
    workspace_editors: Vec<WorkspaceEditorView>,
) -> AnyView {
    let href = format!(
        "/projects/{}/automation/runs/{}/log",
        encode_path(project),
        session.run.id
    );
    let model = session
        .run
        .agent_model
        .clone()
        .unwrap_or_else(|| "default".to_owned());
    let reasoning = session
        .run
        .agent_reasoning_effort
        .map(|effort| effort.to_string())
        .unwrap_or_else(|| "default".to_owned());
    let memory_event = session
        .run
        .memory_event_id
        .map(|event_id| format!("MemoryChanged #{event_id}"));
    let token_usage = run_token_usage_text(&session.run);
    let summary = run_result_summary(&session.run);
    let origin = run_origin_label(&session.run);
    let work_item = run_work_item_link(project, session.run.work_item_id);
    let command = recorded_field(&session.run.command);
    let working_dir = run_workspace_actions(project, &session.run, workspace_editors, href.clone());
    let status_class = run_status_class(session.run.status);
    let output = run_output_view(session.output.clone());
    let prompt = session
        .prompt
        .unwrap_or_else(|| "No prompt file has been written yet.".to_owned());

    view! {
        <article>
            <header class="run-detail-header">
                <div>
                    <h3>"Run #" {session.run.id}</h3>
                    <p>
                        {session.run.status.to_string()}
                        " · "
                        "cleanup "
                        {session.run.cleanup_status}
                    </p>
                </div>
                <a class="button-link secondary-link" href=href>"Open"</a>
            </header>
            <dl class="run-detail-meta">
                {origin.map(|origin| view! {
                    <>
                        <dt>"source"</dt>
                        <dd>{origin}</dd>
                    </>
                })}
                {work_item.map(|work_item| view! {
                    <>
                        <dt>"item"</dt>
                        <dd>{work_item}</dd>
                    </>
                })}
                <dt>"model"</dt>
                <dd>{model}</dd>
                <dt>"reasoning"</dt>
                <dd>{reasoning}</dd>
                <dt>"tokens"</dt>
                <dd>{token_usage}</dd>
                {memory_event.map(|memory_event| view! {
                    <>
                        <dt>"memory"</dt>
                        <dd>{memory_event}</dd>
                    </>
                })}
                <dt>"command"</dt>
                <dd>{command}</dd>
                <dt>"working dir"</dt>
                <dd>{working_dir}</dd>
            </dl>
            <div class=format!("run-result {status_class}")>
                <h4>"Result"</h4>
                <p>{summary}</p>
            </div>
            <div class="run-detail-section">
                <h4>"Prompt"</h4>
                <pre>{prompt}</pre>
            </div>
            <div class="run-detail-section">
                <h4>"Output"</h4>
                {output}
            </div>
        </article>
    }
    .into_any()
}

fn run_output_view(output: Vec<AgentRunOutputPiece>) -> AnyView {
    if output.is_empty() {
        return view! { <p class="muted">"No output has been written yet."</p> }.into_any();
    }
    let pieces = output
        .into_iter()
        .map(run_output_piece_view)
        .collect::<Vec<_>>();
    view! { <div class="run-output">{pieces}</div> }.into_any()
}

fn run_output_piece_view(piece: AgentRunOutputPiece) -> AnyView {
    let kind = piece.kind;
    let kind_class = kind.as_storage().replace('_', "-");
    let sequence = piece.sequence;
    let title = piece.title;
    let body = piece.body;
    let metadata = piece.metadata;
    let badges = output_piece_badges(&metadata);
    let item_id = piece.item_id.map(|item_id| {
        view! {
            <span class="output-piece-id">{item_id}</span>
        }
    });
    let body_view = output_piece_body(kind, body);
    let tool_output = if kind == AgentRunOutputKind::ToolCall {
        tool_output_text(&metadata).map(tool_output_view)
    } else {
        None
    };
    let arguments = if kind == AgentRunOutputKind::ToolCall {
        metadata_value_text(&metadata, "arguments")
            .filter(|value| !value.trim().is_empty())
            .map(|value| expandable_metadata_view("arguments", value))
    } else {
        None
    };

    view! {
        <article class=format!("output-piece output-{kind_class}")>
            <header class="output-piece-header">
                <span class="output-sequence">{"#"}{sequence}</span>
                <strong>{title}</strong>
                {item_id}
                {badges}
            </header>
            {body_view}
            {arguments}
            {tool_output}
        </article>
    }
    .into_any()
}

fn output_piece_body(kind: AgentRunOutputKind, body: String) -> AnyView {
    if body.trim().is_empty() {
        return ().into_any();
    }
    let class = match kind {
        AgentRunOutputKind::ModelMessage => "output-piece-body model-output",
        AgentRunOutputKind::Reasoning => "output-piece-body reasoning-output",
        AgentRunOutputKind::ToolCall | AgentRunOutputKind::FileChange => {
            "output-piece-body tool-call-body"
        }
        AgentRunOutputKind::Error => "output-piece-body error-output",
        AgentRunOutputKind::System | AgentRunOutputKind::Legacy => {
            "output-piece-body system-output"
        }
    };
    view! { <div class=class>{body}</div> }.into_any()
}

fn output_piece_badges(metadata: &serde_json::Value) -> Vec<AnyView> {
    ["tool_type", "status"]
        .into_iter()
        .filter_map(|key| metadata_scalar(metadata, key))
        .map(|value| view! { <span class="output-piece-badge">{value}</span> }.into_any())
        .collect()
}

fn metadata_scalar(metadata: &serde_json::Value, key: &str) -> Option<String> {
    match metadata.get(key)? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn tool_output_text(metadata: &serde_json::Value) -> Option<String> {
    ["output", "result", "content_items", "error"]
        .into_iter()
        .filter_map(|key| metadata_value_text(metadata, key))
        .find(|value| !value.trim().is_empty())
}

fn metadata_value_text(metadata: &serde_json::Value, key: &str) -> Option<String> {
    let value = metadata.get(key)?;
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Array(values) if values.is_empty() => None,
        serde_json::Value::Object(values) if values.is_empty() => None,
        value => serde_json::to_string_pretty(value).ok(),
    }
}

fn tool_output_view(output: String) -> AnyView {
    let (preview, truncated) = abbreviate_chars(&output, TOOL_OUTPUT_PREVIEW_CHARS);
    if truncated {
        view! {
            <details class="tool-output-block">
                <summary>
                    <span class="tool-output-label">"output"</span>
                    <span class="tool-output-preview">{preview}</span>
                </summary>
                <pre class="tool-output-full">{output}</pre>
            </details>
        }
        .into_any()
    } else {
        view! {
            <div class="tool-output-block expanded">
                <span class="tool-output-label">"output"</span>
                <pre class="tool-output-full">{output}</pre>
            </div>
        }
        .into_any()
    }
}

fn expandable_metadata_view(label: &'static str, value: String) -> AnyView {
    let (preview, truncated) = abbreviate_chars(&value, 320);
    if truncated {
        view! {
            <details class="tool-metadata-block">
                <summary>
                    <span>{label}</span>
                    <span>{preview}</span>
                </summary>
                <pre>{value}</pre>
            </details>
        }
        .into_any()
    } else {
        view! {
            <div class="tool-metadata-block compact">
                <span>{label}</span>
                <code>{value}</code>
            </div>
        }
        .into_any()
    }
}

fn abbreviate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let mut chars = value.chars();
    let mut preview = chars.by_ref().take(max_chars).collect::<String>();
    let truncated = chars.next().is_some();
    if truncated {
        preview.push_str("...");
    }
    (preview, truncated)
}

fn maintenance_view(project: &str) -> impl IntoView + 'static {
    let cleanup_action = format!(
        "/projects/{}/automation/cleanup-worktrees",
        encode_path(project)
    );

    view! {
        <section class="maintenance panel">
            <div class="panel-heading">
                <h2>"Maintenance"</h2>
            </div>
            <form method="post" action=cleanup_action>
                <button type="submit">"Cleanup worktrees"</button>
            </form>
        </section>
    }
}

fn create_item_modal(
    api_base_url: String,
    project_id: i64,
    show_when: ReadSignal<bool>,
    set_show_when: WriteSignal<bool>,
    state_options: ReadSignal<Vec<CreateItemStateOption>>,
    selected_state: ReadSignal<String>,
    label_suggestions: Signal<Vec<ProjectLabelView>>,
) -> impl IntoView + 'static {
    let api_base_url = StoredValue::new(api_base_url);
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    let close_modal = Callback::new(move |()| set_show_when.set(false));
    let close_modal_for_exit = close_modal;
    let request_close = Callback::new(move |()| {
        if let Some(context) = context.get_untracked() {
            context.request_leave();
        } else {
            close_modal.run(());
        }
    });
    Effect::new(move |_| {
        if !show_when.get() {
            set_context.set(None);
        }
    });
    let default_create_state = Signal::derive(move || selected_state.get());
    let crud_state_options = Signal::derive(move || state_options.get());
    let request_close_on_escape = request_close;
    let request_close_on_backdrop = request_close;
    let request_close_on_header = request_close;
    let request_close_on_footer = request_close;
    view! {
        <Modal
            id="new-item-modal"
            class="new-item-modal"
            show_when=show_when
            on_escape=move || request_close_on_escape.run(())
            on_backdrop_interaction=move || request_close_on_backdrop.run(())
        >
            <ModalHeader>
                <ModalTitle>"New item"</ModalTitle>
                <button
                    type="button"
                    class="secondary icon-button modal-close-button"
                    title="Close"
                    aria-label="Close"
                    on:click=move |_| request_close_on_header.run(())
                >
                    <Icon icon=icondata::BsX/>
                </button>
            </ModalHeader>
            <ModalBody>
                {move || {
                    if !show_when.get() {
                        return ().into_any();
                    }
                    if state_options.get().is_empty() {
                        return view! {
                            <p class="muted">"No states available."</p>
                        }
                        .into_any();
                    }
                    let api_base_url = api_base_url.get_value();
                    let on_exit = close_modal_for_exit;
                    view! {
                        <div class="new-item-form crudkit-new-item" data-crudkit-leptos="work-item-create">
                            <CrudInstanceMgr>
                                <CrudInstance
                                    name="work-item-create"
                                    config=work_items_crudkit_config_for_view(
                                        api_base_url.clone(),
                                        project_id,
                                        SerializableCrudView::Create,
                                        CrudNavigationConfig::embedded_single_entity()
                                            .with_create_actions_placement(CrudCreateActionsPlacement::External),
                                        default_create_state,
                                        Some(crud_state_options),
                                        label_suggestions,
                                    )
                                    on_exit=on_exit
                                    on_context_created=Callback::new(move |context| {
                                        set_context.set(Some(context));
                                    })
                                />
                            </CrudInstanceMgr>
                        </div>
                    }
                    .into_any()
                }}
            </ModalBody>
            <ModalFooter>
                <button
                    type="button"
                    class="secondary"
                    on:click=move |_| request_close_on_footer.run(())
                >
                    "Cancel"
                </button>
                <CrudCreateActionsOutlet context=context />
            </ModalFooter>
        </Modal>
    }
}

fn board_view(
    project: String,
    items: Vec<WorkItemView>,
    swim_lanes: Vec<SwimLaneView>,
    _work_item_states: Vec<WorkItemStateView>,
    misconfigured_item_count: i64,
    open_create_item: Callback<CreateItemOpenRequest>,
) -> impl IntoView + 'static {
    let lanes = swim_lanes
        .into_iter()
        .map(|lane| {
            let label = lane.name.clone();
            let mut lane_items = items
                .iter()
                .filter(|item| item_matches_condition(item, &lane.filter))
                .cloned()
                .collect::<Vec<_>>();
            sort_lane_items(&mut lane_items, &lane.item_order);
            let cards = lane_items
                .into_iter()
                .map(|item| item_card(project.clone(), item))
                .collect::<Vec<_>>();
            let count = cards.len();
            let create_state = state_identifier_from_lane_filter(&lane.filter);
            let add_button = if lane.can_create_items {
                create_state
                    .map(|create_state| {
                        view! {
                            <button
                                type="button"
                                class="lane-add"
                                on:click=move |_| {
                                    open_create_item.run(CreateItemOpenRequest::SingleState(create_state.clone()))
                                }
                            >
                                "+ Add"
                            </button>
                        }
                        .into_any()
                    })
                    .unwrap_or_else(|| ().into_any())
            } else {
                ().into_any()
            };
            let edit_href = lane_edit_href(&project, lane.id);
            let edit_label = format!("Edit {}", label);
            view! {
                <section class="lane">
                    <header class="lane-header">
                        <h2>{label}</h2>
                        <span class="lane-count">{count}</span>
                        <a
                            class="lane-edit"
                            href=edit_href
                            title=edit_label.clone()
                            aria-label=edit_label
                        >
                            "⚙"
                        </a>
                    </header>
                    <div class="lane-cards">{cards}</div>
                    {add_button}
                </section>
            }
        })
        .collect::<Vec<_>>();
    let warning = if misconfigured_item_count > 0 {
        let item_word = if misconfigured_item_count == 1 {
            "item"
        } else {
            "items"
        };
        let verb = if misconfigured_item_count == 1 {
            "has"
        } else {
            "have"
        };
        let message =
            format!("{misconfigured_item_count} {item_word} {verb} an unknown or missing state.");

        view! {
            <section class="board-state-warning" role="status">
                <strong>"State warning"</strong>
                <span>{message}</span>
                <a href="#work-items-admin">"Review work items"</a>
            </section>
        }
        .into_any()
    } else {
        ().into_any()
    };
    view! {
        <div class="board-stack">
            <section class="board">{lanes}</section>
            {warning}
        </div>
    }
}

fn lane_edit_href(project: &str, lane_id: i64) -> String {
    format!(
        "/projects?project={}&edit_swim_lane={}#swim-lanes",
        encode_path(project),
        lane_id
    )
}

fn item_matches_condition(item: &WorkItemView, condition: &Condition) -> bool {
    match condition {
        Condition::All(elements) => elements
            .iter()
            .all(|element| item_matches_condition_element(item, element)),
        Condition::Any(elements) => elements
            .iter()
            .any(|element| item_matches_condition_element(item, element)),
    }
}

fn item_matches_condition_element(item: &WorkItemView, element: &ConditionElement) -> bool {
    match element {
        ConditionElement::Clause(clause) => item_matches_clause(item, clause),
        ConditionElement::Condition(condition) => item_matches_condition(item, condition),
    }
}

fn item_matches_clause(item: &WorkItemView, clause: &ConditionClause) -> bool {
    let key = clause.column_name.trim();
    let label = item.labels.iter().find(|label| label.key == key);
    let label_value = label.and_then(|label| label.value.as_deref());

    match (&clause.operator, &clause.value) {
        (Operator::Equal, ConditionClauseValue::Bool(expected)) => label.is_some() == *expected,
        (Operator::NotEqual, ConditionClauseValue::Bool(expected)) => label.is_some() != *expected,
        (Operator::Equal, ConditionClauseValue::String(expected)) => {
            label_value == Some(expected.as_str())
        }
        (Operator::NotEqual, ConditionClauseValue::String(expected)) => {
            label_value != Some(expected.as_str())
        }
        (Operator::Equal, ConditionClauseValue::Json(serde_json::Value::Null)) => {
            label.is_some() && label_value.is_none()
        }
        (Operator::NotEqual, ConditionClauseValue::Json(serde_json::Value::Null)) => {
            label.is_none() || label_value.is_some()
        }
        (Operator::IsIn, ConditionClauseValue::Json(serde_json::Value::Array(values))) => {
            let Some(label_value) = label_value else {
                return false;
            };
            values
                .iter()
                .filter_map(|value| value.as_str())
                .any(|expected| expected == label_value)
        }
        _ => false,
    }
}

fn sort_lane_items(items: &mut [WorkItemView], item_order: &str) {
    match item_order {
        "updated_asc" => items.sort_by(|left, right| {
            left.updated_at
                .cmp(&right.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        }),
        "created_desc" => items.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        }),
        "created_asc" => items.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        }),
        "id_desc" => items.sort_by_key(|item| std::cmp::Reverse(item.id)),
        "id_asc" => items.sort_by_key(|item| item.id),
        "title_asc" => items.sort_by(|left, right| {
            left.title
                .to_lowercase()
                .cmp(&right.title.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        }),
        "title_desc" => items.sort_by(|left, right| {
            right
                .title
                .to_lowercase()
                .cmp(&left.title.to_lowercase())
                .then_with(|| right.id.cmp(&left.id))
        }),
        _ => items.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        }),
    }
}

fn item_card(project: String, item: WorkItemView) -> impl IntoView + 'static {
    let href = format!("/projects/{}/items/{}", encode_path(&project), item.id);
    let description = preview(&item.description);
    let claimed = item.claimed_by.is_some();
    let label_chips = item
        .labels
        .iter()
        .map(|label| {
            let blocked = label.key == AUTOMATION_BLOCKED_LABEL_KEY;
            let feedback_requested = label.key == FEEDBACK_REQUESTED_LABEL_KEY;
            let label = format_label(&label.key, label.value.as_deref());
            view! {
                <span
                    class="label-chip"
                    class:blocked=blocked
                    class:feedback=feedback_requested
                >
                    {label}
                </span>
            }
        })
        .collect::<Vec<_>>();
    let claim = item.claimed_by.clone().map(|agent| {
        let status = if item.state.as_deref() == Some("in_progress") {
            "In progress"
        } else {
            "Claimed"
        };
        claim_badge_with_source(
            &project,
            agent,
            status,
            item.claimed_at.clone(),
            item.claim_source.clone(),
        )
    });

    view! {
        <article class="card" class:claimed=claimed>
            <a href=href>
                <h3>{item.title}</h3>
            </a>
            <p>{description}</p>
            <div class="card-labels">{label_chips}</div>
            <footer>
                <span>"v" {item.version}</span>
                <span>{item.comment_count} " comments"</span>
                {claim}
                <span>{item.updated_at}</span>
            </footer>
        </article>
    }
}

fn top_bar(
    projects: Vec<ProjectView>,
    active_project_names: Vec<String>,
    selected_project: Option<String>,
    active: ActivePage,
    automation: Option<TopBarAutomation>,
    codex_status: CodexAppServerStatusView,
) -> impl IntoView + 'static {
    let navigate = use_navigate();
    let selected_query = selected_project
        .as_ref()
        .map(|project| format!("?project={}", encode_path(project)))
        .unwrap_or_default();
    let board_href = if selected_query.is_empty() {
        "/".to_owned()
    } else {
        format!("/{selected_query}")
    };
    let triggers_href = format!("/automation{selected_query}");
    let runs_href = format!("/runs{selected_query}");
    let codex_href = format!("/codex{selected_query}");
    let projects_href = format!("/projects{selected_query}");
    let api_href = format!("/api/docs{selected_query}");
    let board_class = active_class(active, ActivePage::Board);
    let triggers_class = active_class(active, ActivePage::Triggers);
    let runs_class = active_class(active, ActivePage::Runs);
    let projects_class = active_class(active, ActivePage::Projects);
    let api_class = active_class(active, ActivePage::Api);

    let project_options = projects
        .into_iter()
        .map(|project| {
            let active = active_project_names.contains(&project.name);
            ProjectSelectOption {
                name: project.name,
                display_name: project.display_name,
                active,
            }
        })
        .collect::<Vec<_>>();
    let initial_project = project_options
        .iter()
        .find(|project| selected_project.as_deref() == Some(project.name.as_str()))
        .or_else(|| project_options.first())
        .cloned();

    let project_switcher = if let Some(initial_project) = initial_project {
        let (selected_option, set_selected_option) = signal(initial_project);
        let project_options_for_select = project_options.clone();
        view! {
            <div class="project-switcher">
                <span class="project-switcher-label">"Project"</span>
                <Select
                    options=Signal::derive(move || project_options_for_select.clone())
                    search_text_provider=move |option: ProjectSelectOption| {
                        format!("{} {}", option.display_name, option.name)
                    }
                    render_option=project_select_option
                    selected=selected_option
                    set_selected=move |option: ProjectSelectOption| {
                        set_selected_option.set(option.clone());
                        navigate(
                            &format!("/?project={}", encode_path(&option.name)),
                            NavigateOptions::default(),
                        );
                    }
                />
            </div>
        }
        .into_any()
    } else {
        view! {
            <div class="project-switcher project-switcher-empty">
                <span class="project-switcher-label">"Project"</span>
                <span class="project-empty">"No projects"</span>
            </div>
        }
        .into_any()
    };

    let codex_control = top_bar_codex_status(codex_status, codex_href, active == ActivePage::Codex);
    let automation_control = automation.map(top_bar_automation_control);

    view! {
        <header class="app-topbar">
            <a class="brand" href=board_href.clone()>"Patchbay"</a>
            <nav class="top-nav" aria-label="Primary">
                <a class=board_class href=board_href>"Board"</a>
                <a class=triggers_class href=triggers_href>"Automation"</a>
                <a class=runs_class href=runs_href>"Runs"</a>
                <a class=projects_class href=projects_href>"Projects"</a>
                <a class=api_class href=api_href>"API"</a>
            </nav>
            <div class="topbar-actions">{codex_control}{automation_control}</div>
            {project_switcher}
        </header>
    }
}

fn top_bar_codex_status(status: CodexAppServerStatusView, href: String, active: bool) -> AnyView {
    let (tone, label) = if status.usable {
        ("ready", "Ready")
    } else if status.available {
        ("blocked", "Blocked")
    } else {
        ("unavailable", "Unavailable")
    };
    let active_class = if active { " active" } else { "" };
    let class = format!("topbar-codex codex-readiness-{tone}{active_class}");
    let title = status.message;
    let aria_label = format!("Codex automation readiness: {label}");

    view! {
        <a class=class href=href title=title aria-label=aria_label>
            <span class="topbar-codex-dot" aria-hidden="true"></span>
            <span class="topbar-codex-name">"Codex"</span>
            <strong class="topbar-codex-state">{label}</strong>
        </a>
    }
    .into_any()
}

fn top_bar_automation_control(control: TopBarAutomation) -> AnyView {
    let auto_commit_control = top_bar_auto_commit_control(&control);
    if control.running {
        let stop_action = format!(
            "/projects/{}/automation/stop",
            encode_path(&control.project)
        );
        view! {
            <div class="topbar-automation-group">
                {auto_commit_control}
                <form class="topbar-automation" method="post" action=stop_action>
                    <span class="automation-status running">"Running"</span>
                    <button type="submit" class="danger">"Stop"</button>
                </form>
            </div>
        }
        .into_any()
    } else {
        let start_action = format!(
            "/projects/{}/automation/start",
            encode_path(&control.project)
        );
        view! {
            <div class="topbar-automation-group">
                {auto_commit_control}
                <form class="topbar-automation" method="post" action=start_action>
                    <span class="automation-status stopped">"Stopped"</span>
                    <button type="submit">"Start"</button>
                </form>
            </div>
        }
        .into_any()
    }
}

fn top_bar_auto_commit_control(control: &TopBarAutomation) -> Option<AnyView> {
    if control.workspace_mode != WorkspaceMode::CurrentBranch {
        return None;
    }
    let action = format!(
        "/projects/{}/settings/auto-commit",
        encode_path(&control.project)
    );
    let auto_commit = control.auto_commit;
    let set_auto_commit = control.set_auto_commit;
    let (pending, set_pending) = signal(false);
    let (failed, set_failed) = signal(false);
    let form_action = action.clone();
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if pending.get_untracked() {
            return;
        }
        let previous = auto_commit.get_untracked();
        let next = !previous;
        set_auto_commit.set(next);
        set_pending.set(true);
        set_failed.set(false);

        let form_action = form_action.clone();
        leptos::task::spawn_local(async move {
            if post_auto_commit_update(form_action, next).await {
                set_pending.set(false);
            } else {
                set_auto_commit.set(previous);
                set_pending.set(false);
                set_failed.set(true);
            }
        });
    };

    Some(
        view! {
            <form class="topbar-auto-commit-form" method="post" action=action on:submit=submit>
                <input type="hidden" name="enabled" value=move || (!auto_commit.get()).to_string()/>
                <button
                    type="submit"
                    class=move || {
                        let mut class = if auto_commit.get() {
                            "topbar-auto-commit enabled".to_owned()
                        } else {
                            "topbar-auto-commit".to_owned()
                        };
                        if pending.get() {
                            class.push_str(" pending");
                        }
                        if failed.get() {
                            class.push_str(" failed");
                        }
                        class
                    }
                    role="switch"
                    aria-checked=move || auto_commit.get().to_string()
                    title=move || {
                        if pending.get() {
                            "Saving Auto-Commit setting".to_owned()
                        } else if auto_commit.get() {
                            "Turn Auto-Commit off".to_owned()
                        } else {
                            "Turn Auto-Commit on".to_owned()
                        }
                    }
                    disabled=move || pending.get()
                >
                    <span class="auto-commit-label">"Auto-Commit"</span>
                    <span class="auto-commit-track" aria-hidden="true">
                        <span class="auto-commit-thumb"></span>
                    </span>
                    <strong>{move || if auto_commit.get() { "On" } else { "Off" }}</strong>
                </button>
            </form>
        }
        .into_any(),
    )
}

#[cfg(not(feature = "ssr"))]
async fn post_auto_commit_update(action: String, enabled: bool) -> bool {
    let request = match gloo_net::http::Request::post(&action)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("x-patchbay-background", "true")
        .body(format!("enabled={enabled}"))
    {
        Ok(request) => request,
        Err(_) => return false,
    };

    request
        .send()
        .await
        .map(|response| response.ok())
        .unwrap_or(false)
}

#[cfg(feature = "ssr")]
async fn post_auto_commit_update(_action: String, _enabled: bool) -> bool {
    false
}

fn project_select_option(option: ProjectSelectOption) -> AnyView {
    view! {
        <span class="project-option">
            <span
                class="project-option-dot"
                class:active=option.active
                aria-hidden="true"
            ></span>
            <span class="project-option-name">{option.display_name}</span>
        </span>
    }
    .into_any()
}

fn active_class(active: ActivePage, page: ActivePage) -> &'static str {
    if active == page { "active" } else { "" }
}

fn encode_path(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

fn item_href(project: &str, item_id: i64) -> String {
    format!("/projects/{}/items/{}", encode_path(project), item_id)
}

fn state_label(item: &WorkItemView) -> &str {
    item.state.as_deref().unwrap_or("(no state)")
}

fn claim_badge(
    project: &str,
    agent: String,
    status: &'static str,
    claimed_at: Option<String>,
) -> AnyView {
    claim_badge_with_source(project, agent, status, claimed_at, None)
}

fn claim_badge_with_source(
    project: &str,
    agent: String,
    status: &'static str,
    claimed_at: Option<String>,
    claim_source: Option<WorkItemClaimSourceView>,
) -> AnyView {
    let elapsed = claim_elapsed_timer(claimed_at);
    let source_label = claim_source_label(claim_source.as_ref());
    let run_id = claim_source
        .as_ref()
        .map(|source| source.run_id)
        .or_else(|| infer_patchbay_run_id(&agent));
    if let Some(run_id) = run_id {
        let href = format!(
            "/projects/{}/automation/runs/{}/log",
            encode_path(project),
            run_id
        );
        return view! {
            <a class="claim-badge" href=href>
                <span class="claim-dot" aria-hidden="true"></span>
                <span>{status}</span>
                <span class="claim-agent">{agent}</span>
                {source_label.map(|source| view! {
                    <span class="claim-source" title="Automation source">{source}</span>
                })}
                {elapsed}
            </a>
        }
        .into_any();
    }

    view! {
        <div class="claim-badge">
            <span class="claim-dot" aria-hidden="true"></span>
            <span>{status}</span>
            <span class="claim-agent">{agent}</span>
            {source_label.map(|source| view! {
                <span class="claim-source" title="Automation source">{source}</span>
            })}
            {elapsed}
        </div>
    }
    .into_any()
}

fn claim_source_label(source: Option<&WorkItemClaimSourceView>) -> Option<String> {
    source.map(|source| {
        source
            .trigger_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| format!("via {name}"))
            .unwrap_or_else(|| "via direct run".to_owned())
    })
}

fn claim_elapsed_timer(claimed_at: Option<String>) -> AnyView {
    let Some(claimed_at) = claimed_at else {
        return ().into_any();
    };
    if claim_elapsed_seconds(&claimed_at).is_none() {
        return ().into_any();
    }

    let (tick, set_tick) = signal(0_u64);
    let _poll = use_interval_fn(
        move || {
            set_tick.update(|tick| *tick = tick.saturating_add(1));
        },
        1000,
    );
    view! {
        <span class="claim-elapsed" title="Time in progress">
            {move || {
                let _ = tick.get();
                claim_elapsed_label(&claimed_at).unwrap_or_default()
            }}
        </span>
    }
    .into_any()
}

fn claim_elapsed_label(claimed_at: &str) -> Option<String> {
    claim_elapsed_seconds(claimed_at).map(format_claim_elapsed_seconds)
}

fn claim_elapsed_seconds(claimed_at: &str) -> Option<i64> {
    claim_elapsed_seconds_at(claimed_at, OffsetDateTime::now_utc())
}

fn claim_elapsed_seconds_at(claimed_at: &str, now: OffsetDateTime) -> Option<i64> {
    let claimed_at = OffsetDateTime::parse(claimed_at, &Rfc3339).ok()?;
    Some((now - claimed_at).whole_seconds().max(0))
}

fn format_claim_elapsed_seconds(total_seconds: i64) -> String {
    let total_seconds = total_seconds.max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_label(key: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("{key}={value}"),
        None => key.to_owned(),
    }
}

fn preview(value: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 140;
    let value = rich_text_plain_text(value);
    if value.chars().count() <= MAX_PREVIEW_CHARS {
        return value;
    }

    value.chars().take(MAX_PREVIEW_CHARS).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use crate::frontend::rich_text::rich_text_editor_html;

    use super::{
        UiEvent, claim_elapsed_seconds_at, format_claim_elapsed_seconds, infer_patchbay_run_id,
        preview, runs_page_event_matches,
    };
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    #[test]
    fn infers_run_id_from_patchbay_agent_name() {
        assert_eq!(infer_patchbay_run_id("patchbay-run-60"), Some(60));
    }

    #[test]
    fn ignores_non_run_agent_names() {
        assert_eq!(infer_patchbay_run_id("codex"), None);
        assert_eq!(infer_patchbay_run_id("patchbay-run-"), None);
        assert_eq!(infer_patchbay_run_id("patchbay-run-0"), None);
        assert_eq!(infer_patchbay_run_id("patchbay-run-+60"), None);
        assert_eq!(infer_patchbay_run_id("patchbay-run-abc"), None);
    }

    #[test]
    fn formats_claim_elapsed_time() {
        assert_eq!(format_claim_elapsed_seconds(70), "1:10");
        assert_eq!(format_claim_elapsed_seconds(3670), "1:01:10");
        assert_eq!(format_claim_elapsed_seconds(-5), "0:00");
    }

    #[test]
    fn derives_claim_elapsed_time_from_claim_timestamp() {
        let now = OffsetDateTime::parse("2026-06-17T18:01:10Z", &Rfc3339).unwrap();
        assert_eq!(
            claim_elapsed_seconds_at("2026-06-17T18:00:00Z", now),
            Some(70)
        );
        assert_eq!(
            claim_elapsed_seconds_at("2026-06-17T18:02:00Z", now),
            Some(0)
        );
        assert_eq!(claim_elapsed_seconds_at("not a timestamp", now), None);
    }

    #[test]
    fn rich_text_editor_html_preserves_plain_text_line_breaks() {
        assert_eq!(
            rich_text_editor_html("First line\nSecond line\n\nThird"),
            "<p>First line<br>Second line</p><p>Third</p>"
        );
    }

    #[test]
    fn preview_omits_rich_text_markup() {
        assert_eq!(
            preview("<p>First <strong>item</strong></p><p>Second</p>"),
            "First item\nSecond"
        );
    }

    #[test]
    fn runs_page_shell_ignores_live_run_events() {
        assert!(!runs_page_event_matches(&UiEvent::AutomationChanged {
            sequence: 1,
            timestamp: "2026-06-18T00:00:00Z".to_owned(),
            project: "demo".to_owned(),
        }));
        assert!(!runs_page_event_matches(&UiEvent::AgentRunChanged {
            sequence: 2,
            timestamp: "2026-06-18T00:00:01Z".to_owned(),
            project: "demo".to_owned(),
            run_id: 42,
            item_id: Some(7),
        }));
        assert!(!runs_page_event_matches(&UiEvent::AgentOutputChanged {
            sequence: 3,
            timestamp: "2026-06-18T00:00:02Z".to_owned(),
            project: "demo".to_owned(),
            run_id: 42,
            item_id: Some(7),
        }));
    }

    #[test]
    fn runs_page_shell_refreshes_for_shell_context_events() {
        assert!(runs_page_event_matches(&UiEvent::ProjectListChanged {
            sequence: 1,
            timestamp: "2026-06-18T00:00:00Z".to_owned(),
        }));
        assert!(runs_page_event_matches(&UiEvent::ProjectChanged {
            sequence: 2,
            timestamp: "2026-06-18T00:00:01Z".to_owned(),
            project: "demo".to_owned(),
        }));
        assert!(runs_page_event_matches(&UiEvent::CodexStatusChanged {
            sequence: 3,
            timestamp: "2026-06-18T00:00:02Z".to_owned(),
        }));
    }
}
