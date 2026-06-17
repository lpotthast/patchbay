use std::sync::Arc;

#[cfg(feature = "ssr")]
use crate::backend::{app_state, page_data};
use crate::{
    frontend::routes::routes,
    frontend::types::{
        agent_tool::{
            AgentTool, AgentToolField, CreateAgentTool, CreateAgentToolField,
            CrudAgentToolResource, ReadAgentTool, ReadAgentToolField,
        },
        automation_trigger::{
            AutomationTrigger, AutomationTriggerField, CreateAutomationTrigger,
            CreateAutomationTriggerField, CrudAutomationTriggerResource, ReadAutomationTrigger,
            ReadAutomationTriggerField,
        },
        project::{
            CreateProject, CreateProjectField, CrudProjectResource, Project as CrudProject,
            ProjectField, ReadProject, ReadProjectField,
        },
        swim_lane::{
            CreateSwimLane, CreateSwimLaneField, CrudSwimLaneResource, ReadSwimLane,
            ReadSwimLaneField, SwimLane, SwimLaneField,
        },
        work_item::{
            CreateWorkItem as CrudCreateWorkItem, CreateWorkItemField, CrudWorkItemResource,
            ReadWorkItem, ReadWorkItemField, WorkItem as CrudWorkItem, WorkItemField,
        },
    },
    shared::view_models::{
        AUTOMATION_BLOCKED_LABEL_KEY, AgentReasoningEffort, AgentRunOutputKind,
        AgentRunOutputPiece, AgentRunStatus, AgentRunView, AuthorType, AutomationStatusView,
        CLAIMED_FROM_STATE_LABEL_KEY, CodexAgentModel, CodexAppServerStatusView,
        CodexAuthSetupView, CodexRateLimitView, CodexUsageSummaryView, CommentView,
        ProjectLabelView, ProjectMemoryEventRefView, ProjectMemoryEventView, ProjectSettingsView,
        ProjectView, RunLogView, STATE_LABEL_KEY, SwimLaneView, UiEvent, WorkItemLabelView,
        WorkItemView,
    },
};
#[cfg(not(feature = "ssr"))]
use codee::string::FromToStringCodec;
use crudkit_leptos::crud_instance::CrudInstanceContext;
use crudkit_leptos::crud_instance_mgr::CrudInstanceMgr;
use crudkit_leptos::crudkit_core::{
    Value,
    condition::{Condition, ConditionClause, ConditionClauseValue, ConditionElement, Operator},
    id::{IdValue, SerializableId},
};
use crudkit_leptos::fields::{FieldRenderer, render_label};
use crudkit_leptos::{
    crud_instance_config::{FieldRendererRegistry, Header, ItemsPerPage, ModelHandler, PageNr},
    crudkit_web::{
        HeaderOptions, Label, reqwest_executor::NewClientPerRequestExecutor,
        view::SerializableCrudView,
    },
    prelude::*,
};
use indexmap::indexmap;
use leptonic::components::prelude::{
    LeptonicTheme, Modal, ModalBody, ModalFooter, ModalHeader, ModalTitle, Root, Select,
};
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
#[cfg(not(feature = "ssr"))]
use leptos_use::{
    ReconnectLimit, UseWebSocketOptions, UseWebSocketReturn, use_websocket_with_options,
};
use serde::{Deserialize, Serialize};

const TOOL_OUTPUT_PREVIEW_CHARS: usize = 1200;
const BOARD_ITEMS_REFRESH_INTERVAL_MS: u64 = 30_000;
const DEFAULT_CREATE_ITEM_STATE: &str = "idea";

#[derive(Clone, Debug, PartialEq, Eq)]
struct CreateItemLaneOption {
    identifier: String,
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CreateItemOpenRequest {
    AnyCreatableLane,
    SingleLane(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BoardPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub selected_project: Option<String>,
    pub selected_project_view: Option<ProjectView>,
    pub settings: Option<ProjectSettingsView>,
    pub memory_events: Vec<ProjectMemoryEventView>,
    pub automation_status: Option<AutomationStatusView>,
    pub automation_running: bool,
    pub run_sessions: Vec<BoardRunSessionView>,
    pub items: Vec<WorkItemView>,
    pub swim_lanes: Vec<SwimLaneView>,
    pub misconfigured_item_count: i64,
    pub api_base_url: String,
    pub codex_status: CodexAppServerStatusView,
    pub runtime: RuntimeConfigView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BoardItemsSection {
    pub items: Vec<WorkItemView>,
    pub swim_lanes: Vec<SwimLaneView>,
    pub misconfigured_item_count: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BoardAutomationSection {
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
    pub label_suggestions: Vec<ProjectLabelView>,
    pub automation_runs: Vec<AgentRunView>,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunLogPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub project: String,
    pub run_log: RunLogView,
    pub codex_status: CodexAppServerStatusView,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TriggersPage {
    pub projects: Vec<ProjectView>,
    pub active_project_names: Vec<String>,
    pub selected_project: Option<String>,
    pub selected_project_view: Option<ProjectView>,
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum ActivePage {
    Board,
    Triggers,
    Codex,
    Projects,
    Api,
}

#[derive(Clone)]
struct TopBarAutomation {
    project: String,
    running: bool,
}

#[derive(Clone, Copy)]
struct LiveEventContext {
    latest_event: ReadSignal<Option<UiEvent>>,
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
    view! {
        <CrudInstanceMgr>
            <LiveEventsProvider/>
            <Outlet/>
        </CrudInstanceMgr>
    }
}

#[component]
fn LiveEventsProvider() -> impl IntoView {
    let (latest_event, set_latest_event) = signal(None::<UiEvent>);
    provide_context(LiveEventContext { latest_event });
    #[cfg(feature = "ssr")]
    let _ = set_latest_event;

    #[cfg(not(feature = "ssr"))]
    {
        let UseWebSocketReturn { message, .. } =
            use_websocket_with_options::<String, String, FromToStringCodec, _, _>(
                "/api/events/ws",
                UseWebSocketOptions::default()
                    .reconnect_limit(ReconnectLimit::Infinite)
                    .reconnect_interval(1_000),
            );
        Effect::new(move |_| {
            if let Some(raw) = message.get()
                && let Ok(event) = serde_json::from_str::<UiEvent>(&raw)
            {
                set_latest_event.set(Some(event));
            }
        });
    }
}

#[component]
pub fn PageBoard() -> impl IntoView {
    let selected_project = selected_project_signal();
    let api_base_url = api_base_url();
    let page =
        LocalResource::new(move || load_board_page(selected_project.get(), api_base_url.clone()));

    view! {
        <Title text="Patchbay"/>
        {move || page_view(page.get(), board_content)}
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
        &state.sessions,
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

#[server(prefix = "/leptos")]
async fn load_board_automation_section(
    project: String,
) -> Result<BoardAutomationSection, ServerFnError> {
    let state = app_state::app_state();
    page_data::board_automation_section(
        &state.store,
        &state.sessions,
        &state.automation_controller,
        &project,
    )
    .await
    .map_err(|err| ServerFnError::new(err.to_string()))
}

#[component]
pub fn PageProjects() -> impl IntoView {
    let selected_project = selected_project_signal();
    let api_base_url = api_base_url();
    let api_base_url_for_panel = api_base_url.clone();
    let page = LocalResource::new(move || {
        load_projects_page(selected_project.get(), api_base_url.clone())
    });

    view! {
        <Title text="Projects"/>
        {move || page_view(page.get(), projects_content)}
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
    let api_base_url = api_base_url();
    let page = LocalResource::new(move || {
        load_triggers_page(selected_project.get(), api_base_url.clone())
    });

    view! {
        <Title text="Automation"/>
        {move || page_view(page.get(), triggers_content)}
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
pub fn PageCodex() -> impl IntoView {
    let selected_project = selected_project_signal();
    let page = LocalResource::new(move || load_codex_status_page(selected_project.get()));
    refetch_on_live_event(page, codex_event_matches);

    view! {
        <Title text="Codex automation"/>
        {move || page_view(page.get(), codex_status_content)}
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
    let project_for_loader = project.clone();
    let project_for_events = project;
    let page = LocalResource::new(move || load_item_page(project_for_loader.clone(), item_id));
    refetch_on_live_event(page, move |event| {
        item_event_matches(event, project_for_events.clone(), item_id)
    });

    view! {
        <Title text="Patchbay"/>
        {move || page_view(page.get(), item_content)}
    }
}

#[server(prefix = "/leptos")]
async fn load_item_page(
    project: Option<String>,
    item_id: Option<i64>,
) -> Result<ItemPage, ServerFnError> {
    let state = app_state::app_state();
    let codex_status = state.codex_status.read().await.clone();
    match (project, item_id) {
        (Some(project), Some(item_id)) => page_data::item_page_data(
            &state.store,
            &state.automation_controller,
            &project,
            item_id,
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
    let project_for_loader = project.clone();
    let project_for_events = project;
    let page = LocalResource::new(move || load_run_log_page(project_for_loader.clone(), run_id));
    refetch_on_live_event(page, move |event| {
        run_log_event_matches(event, project_for_events.clone(), run_id)
    });

    view! {
        <Title text="Run log"/>
        {move || page_view(page.get(), run_log_content)}
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
    let page = LocalResource::new(move || load_api_docs_page(selected_project.get()));
    refetch_on_live_event(page, api_docs_event_matches);

    view! {
        <Title text="Patchbay API"/>
        {move || page_view(page.get(), api_docs_content)}
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

fn refetch_on_live_event<T>(
    resource: LocalResource<Result<T, ServerFnError>>,
    should_refetch: impl Fn(&UiEvent) -> bool + 'static,
) where
    T: 'static,
{
    if let Some(context) = use_context::<LiveEventContext>() {
        Effect::new(move |_| {
            if let Some(event) = context.latest_event.get()
                && should_refetch(&event)
            {
                resource.refetch();
            }
        });
    }
}

fn reload_crudkit_on_live_event(
    context: ReadSignal<Option<CrudInstanceContext>>,
    should_reload: impl Fn(&UiEvent) -> bool + 'static,
) {
    if let Some(live) = use_context::<LiveEventContext>() {
        Effect::new(move |_| {
            if let Some(event) = live.latest_event.get()
                && should_reload(&event)
                && let Some(context) = context.get()
            {
                context.reload();
            }
        });
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
        | UiEvent::MemoryChanged { .. }
        | UiEvent::SwimLaneChanged { .. } => false,
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
        | UiEvent::MemoryChanged { .. }
        | UiEvent::SwimLaneChanged { .. } => false,
    }
}

fn event_scopes_named_project(event: &UiEvent, project: Option<&str>) -> bool {
    match (project, event_project(event)) {
        (Some(expected), Some(actual)) => expected == actual,
        (Some(_), None) => true,
        (None, _) => true,
    }
}

fn event_project(event: &UiEvent) -> Option<&str> {
    match event {
        UiEvent::ProjectChanged { project, .. }
        | UiEvent::WorkItemChanged { project, .. }
        | UiEvent::CommentChanged { project, .. }
        | UiEvent::MemoryChanged { project, .. }
        | UiEvent::SwimLaneChanged { project, .. }
        | UiEvent::AutomationChanged { project, .. }
        | UiEvent::AgentRunChanged { project, .. }
        | UiEvent::AgentOutputChanged { project, .. } => Some(project),
        UiEvent::ProjectListChanged { .. }
        | UiEvent::AgentToolChanged { .. }
        | UiEvent::CodexStatusChanged { .. } => None,
    }
}

fn error_message_from_query() -> Option<String> {
    use_query_map().read_untracked().get("message")
}

fn selected_trigger_id_from_context(context: CrudInstanceContext) -> Option<i64> {
    match context.view.get() {
        SerializableCrudView::Read(id) | SerializableCrudView::Edit(id) => serializable_i64_id(&id),
        SerializableCrudView::List | SerializableCrudView::Create => None,
    }
}

fn serializable_i64_id(id: &SerializableId) -> Option<i64> {
    id.entries().find_map(|entry| match &entry.value {
        IdValue::I64(value) => Some(*value),
        IdValue::I32(value) => Some(i64::from(*value)),
        IdValue::I16(value) => Some(i64::from(*value)),
        IdValue::I8(value) => Some(i64::from(*value)),
        _ => None,
    })
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

fn page_view<T>(
    page: Option<Result<T, ServerFnError>>,
    content: impl FnOnce(T) -> AnyView,
) -> AnyView {
    match page {
        Some(Ok(page)) => content(page),
        Some(Err(err)) => error_content(err.to_string()),
        None => page_loading().into_any(),
    }
}

fn board_content(page: BoardPage) -> AnyView {
    let BoardPage {
        projects,
        active_project_names,
        selected_project,
        selected_project_view,
        settings,
        memory_events,
        automation_status,
        automation_running,
        run_sessions,
        items,
        swim_lanes,
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
        let topbar = top_bar(
            projects,
            active_project_names,
            Some(project.clone()),
            ActivePage::Board,
            Some(TopBarAutomation {
                project: project.clone(),
                running: automation_running || automation_status.running_runs > 0,
            }),
            codex_status.clone(),
        );
        let page_title = project_view.display_name.clone();
        let board_return_to = format!("/?project={}", encode_path(&project));
        let project_workspace =
            project_workspace_panel(&project, &project_view, board_return_to.clone());
        let (show_create_item_modal, set_show_create_item_modal) = signal(false);
        let initial_create_item_lane_options = creatable_lane_options(&swim_lanes);
        let initial_create_item_state =
            default_create_item_state(&initial_create_item_lane_options);
        let (create_item_state, set_create_item_state) = signal(initial_create_item_state);
        let (create_item_lane_options, set_create_item_lane_options) =
            signal(initial_create_item_lane_options);
        let (create_item_swim_lanes, set_create_item_swim_lanes) = signal(swim_lanes.clone());
        let has_create_item_lanes =
            Memo::new(move |_| !creatable_lane_options(&create_item_swim_lanes.get()).is_empty());
        let open_create_item = Callback::new(move |request: CreateItemOpenRequest| {
            let lanes = create_item_swim_lanes.get_untracked();
            let options = create_item_options_for_request(&lanes, &request);
            if options.is_empty() {
                return;
            }
            set_create_item_state.set(default_create_item_state(&options));
            set_create_item_lane_options.set(options);
            set_show_create_item_modal.set(true);
        });
        let board = view! {
            <LiveBoardItems
                project=project.clone()
                initial_items=items
                initial_swim_lanes=swim_lanes
                initial_misconfigured_item_count=misconfigured_item_count
                open_create_item=open_create_item
                set_create_item_swim_lanes=set_create_item_swim_lanes
            />
        };
        let create_item = create_item_modal(
            &project,
            show_create_item_modal,
            set_show_create_item_modal,
            create_item_lane_options,
            create_item_state,
            set_create_item_state,
        );
        let work_items_api_base_url = api_base_url.clone();
        let swim_lanes_api_base_url = api_base_url;
        let admin_project_id = project_view.id;
        let project_settings =
            project_settings_view(&project, project_view, settings, memory_events);
        let automation_view = view! {
            <LiveBoardAutomation
                project=project.clone()
                initial_status=automation_status
                initial_running=automation_running
                initial_run_sessions=run_sessions
            />
        };
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
                            disabled=move || !has_create_item_lanes.get()
                            on:click=move |_| {
                                open_create_item.run(CreateItemOpenRequest::AnyCreatableLane)
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
                    <SwimLanesPanel
                        api_base_url=swim_lanes_api_base_url
                        project=project.clone()
                        project_id=admin_project_id
                    />
                    {project_settings}
                    {automation_view}
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
        let trigger_runs = trigger_runs_panel(project.clone(), selected_trigger_id);
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
    let topbar = top_bar(
        projects.clone(),
        active_project_names,
        selected_project,
        ActivePage::Projects,
        None,
        codex_status,
    );

    view! {
        <div>
            {topbar}
            <main class="page-shell projects-page">
                <section class="page-heading">
                    <h1>"Projects"</h1>
                </section>
                {projects_panel(api_base_url)}
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

fn item_content(page: ItemPage) -> AnyView {
    let ItemPage {
        projects,
        active_project_names,
        project,
        item,
        comments,
        label_suggestions,
        automation_runs,
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
    let delete_action = format!(
        "/projects/{}/items/{}/delete",
        encode_path(&project),
        item.id
    );
    let automation_action = format!("/projects/{}/automation/start", encode_path(&project));
    let update_action = format!(
        "/projects/{}/items/{}/update",
        encode_path(&project),
        item.id
    );
    let comment_action = format!(
        "/projects/{}/items/{}/comments",
        encode_path(&project),
        item.id
    );
    let update_title = item.title.clone();
    let update_description = item.description.clone();
    let header_title = item.title.clone();
    let header_description = item.description.clone();
    let item_state_display = state_label(&item).to_owned();
    let model_override_options =
        agent_model_options(item.agent_model_override.clone(), "Project default");
    let reasoning_override_options =
        agent_reasoning_options(item.agent_reasoning_effort_override, "Project default");
    let state_action = format!("/projects/{}/items/{}/move", encode_path(&project), item.id);
    let current_state = item.state.clone().unwrap_or_default();
    let claim = item.claimed_by.clone().map(|agent| {
        view! { <span>"claimed by " {agent}</span> }
    });
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

    view! {
        <div>
            {topbar}
            <main class="page-shell item-page">
                <section class="item-header">
                    <a href=board_href>"Board"</a>
                    <h1>{header_title}</h1>
                    <p>{header_description}</p>
                </section>
                <section class="item-meta">
                    <span>{item_state_display}</span>
                    <span>"v" {item.version}</span>
                    {claim}
                    {finished}
                </section>
                <section class="item-settings panel">
                    <h2>"Item details"</h2>
                    <form method="post" action=update_action>
                        <input type="hidden" name="version" value=item.version.to_string()/>
                        <label>
                            <span>"Title"</span>
                            <input name="title" value=update_title required/>
                        </label>
                        <label>
                            <span>"Description"</span>
                            <textarea name="description" required>{update_description}</textarea>
                        </label>
                        <label>
                            <span>"Agent model override"</span>
                            <select name="agent_model_override">
                                {model_override_options}
                            </select>
                        </label>
                        <label>
                            <span>"Reasoning override"</span>
                            <select name="agent_reasoning_effort_override">
                                {reasoning_override_options}
                            </select>
                        </label>
                        <button>"Save item"</button>
                    </form>
                </section>
                <section class="actions">
                    <form method="post" action=state_action>
                        <input type="hidden" name="version" value=item.version.to_string()/>
                        <input
                            name="state"
                            value=current_state
                            placeholder="state label"
                            required
                        />
                        <button>"Set state"</button>
                    </form>
                    <form method="post" action=delete_action>
                        <button class="danger">"Delete"</button>
                    </form>
                    <form method="post" action=automation_action>
                        <input type="hidden" name="item_id" value=item.id.to_string()/>
                        <select name="mode">
                            <option value="execute">"execute"</option>
                            <option value="refine">"refine"</option>
                        </select>
                        <button>"Start agent"</button>
                    </form>
                </section>
                {labels}
                {automation_run_views}
                <section class="comments">
                    <h2>"Comments"</h2>
                    {comment_views}
                    <form method="post" action=comment_action>
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
        && let Some(run_id) = infer_agent_comment_run_id(&author)
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

fn infer_agent_comment_run_id(author: &str) -> Option<i64> {
    let id = author.strip_prefix("patchbay-run-")?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let run_id = id.parse::<i64>().ok()?;
    (run_id > 0).then_some(run_id)
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
    let state_suggestion_options = state_suggestion_options(&suggestions);
    let rows = item
        .labels
        .iter()
        .cloned()
        .map(|label| item_label_row(project, item, label))
        .collect::<Vec<_>>();

    view! {
        <section class="item-labels panel">
            <h2>"Labels"</h2>
            <datalist id="label-key-suggestions">{suggestion_options}</datalist>
            <datalist id="state-value-suggestions">{state_suggestion_options}</datalist>
            <div class="label-list">{rows}</div>
            <form class="label-add-form" method="post" action=add_action>
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
    let can_delete = label.key != STATE_LABEL_KEY;
    let blocked = label.key == AUTOMATION_BLOCKED_LABEL_KEY;

    view! {
        <article class="label-row">
            <span class="label-chip" class:blocked=blocked>{rendered}</span>
            <form method="post" action=update_action>
                <input type="hidden" name="version" value=item.version.to_string()/>
                <input name="key" value=label.key required/>
                <input name="value" value=value/>
                <button>"Update"</button>
            </form>
            {can_delete.then(|| view! {
                <form method="post" action=delete_action>
                    <input type="hidden" name="version" value=item.version.to_string()/>
                    <button class="danger">"Delete"</button>
                </form>
            })}
        </article>
    }
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

fn state_suggestion_options(suggestions: &[ProjectLabelView]) -> Vec<impl IntoView> {
    suggestions
        .iter()
        .filter(|label| label.key == STATE_LABEL_KEY)
        .filter_map(|label| label.value.clone())
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
            view! {
                <li>
                    <a href=href>"#" {run.id}</a>
                    " · "
                    {run.status.to_string()}
                    " · "
                    {run.mode.to_string()}
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
    let command = recorded_field(&run_log.run.command);
    let run_href = format!(
        "/projects/{}/automation/runs/{}/log",
        encode_path(&project),
        run_log.run.id
    );
    let working_dir = run_workspace_actions(&project, &run_log.run, run_href);
    let status_class = run_status_class(run_log.run.status);
    let memory_event = run_log.memory_event.as_ref().map(memory_event_ref_label);
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
                        {run_log.run.mode.to_string()}
                        " · "
                        {summary.clone()}
                    </p>
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
                        <dt>"result"</dt>
                        <dd class=format!("run-result-inline {status_class}")>{summary}</dd>
                        <dt>"command"</dt>
                        <dd>{command}</dd>
                        <dt>"working dir"</dt>
                        <dd>{working_dir}</dd>
                        <dt>"cleanup"</dt>
                        <dd>{run_log.run.cleanup_status}</dd>
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
                    <h2>"Output"</h2>
                    {output}
                </section>
                <section>
                    <h2>"Prompt"</h2>
                    <pre>{prompt}</pre>
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
        "GET /api/projects/{project}/automation/sessions",
        "POST /projects/{project}/automation/start",
        "POST /projects/{project}/automation/stop",
        "POST /projects/{project}/automation/recover-stale-claims",
        "POST /projects/{project}/automation/cleanup-worktrees",
        "POST /projects/{project}/workspace/open",
        "POST /projects/{project}/automation/runs/{run_id}/workspace/open",
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

fn projects_panel(api_base_url: String) -> impl IntoView + 'static {
    view! {
        <section class="project-management panel">
            <div class="panel-heading">
                <h2>"Projects"</h2>
            </div>
            <div class="crudkit-projects" data-crudkit-leptos="projects">
                {projects_crudkit_instance(api_base_url)}
            </div>
        </section>
    }
}

fn projects_crudkit_instance(api_base_url: String) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, |event| {
        matches!(
            event,
            UiEvent::ProjectListChanged { .. } | UiEvent::ProjectChanged { .. }
        )
    });

    view! {
        <CrudInstance
            name="projects"
            config=projects_crudkit_config(api_base_url)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn projects_crudkit_config(api_base_url: String) -> CrudInstanceConfig {
    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns: vec![
            Header::showing(
                ReadProjectField::Id,
                HeaderOptions {
                    display_name: "#".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::Name,
                HeaderOptions {
                    display_name: "Project key".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::DisplayName,
                HeaderOptions {
                    display_name: "Display name".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::Path,
                HeaderOptions {
                    display_name: "Path".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::PathExists,
                HeaderOptions {
                    display_name: "Path status".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::WorkspaceMode,
                HeaderOptions {
                    display_name: "Workspace".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::DefaultAgentModel,
                HeaderOptions {
                    display_name: "Model".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::DefaultAgentReasoningEffort,
                HeaderOptions {
                    display_name: "Reasoning".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::UpdatedAt,
                HeaderOptions {
                    display_name: "Updated".into(),
                    ..Default::default()
                },
            ),
        ],
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::create_field(
                    CreateProjectField::Name,
                    FieldOptions {
                        label: Some(Label::new("Project key")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::DisplayName,
                    FieldOptions {
                        label: Some(Label::new("Display name")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::Path,
                    FieldOptions {
                        label: Some(Label::new("Path")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::DefaultAgentModel,
                    FieldOptions {
                        label: Some(Label::new("Default model")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::DefaultAgentReasoningEffort,
                    FieldOptions {
                        label: Some(Label::new("Default reasoning")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::Memory,
                    FieldOptions {
                        label: Some(Label::new("Memory")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    CrudProject::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::DisplayName,
                    FieldOptions {
                        label: Some(Label::new("Display name")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::Path,
                    FieldOptions {
                        label: Some(Label::new("Path")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::Memory,
                    FieldOptions {
                        label: Some(Label::new("Memory")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::WorkspaceMode,
                    FieldOptions {
                        label: Some(Label::new("Workspace")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::MaxCodeEditAgents,
                    FieldOptions {
                        label: Some(Label::new("Max agents")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::AllowRefinementAgentsDuringEditing,
                    FieldOptions {
                        label: Some(Label::new("Allow refinement while editing")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::CreatePr,
                    FieldOptions {
                        label: Some(Label::new("Create PR")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::StaleClaimMinutes,
                    FieldOptions {
                        label: Some(Label::new("Stale claim minutes")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::WorktreeCleanupPolicy,
                    FieldOptions {
                        label: Some(Label::new("Worktree cleanup")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::DefaultAgentTool,
                    FieldOptions {
                        label: Some(Label::new("Default tool")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::DefaultAgentModel,
                    FieldOptions {
                        label: Some(Label::new("Default model")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::DefaultAgentReasoningEffort,
                    FieldOptions {
                        label: Some(Label::new("Default reasoning")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::AgentSandboxMode,
                    FieldOptions {
                        label: Some(Label::new("Sandbox mode")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::AgentExtraWritableRoots,
                    FieldOptions {
                        label: Some(Label::new("Extra writable roots")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadProject::Name.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: None,
        resource_name: CrudProjectResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: ModelHandler::new::<CreateProject, ReadProject, CrudProject>(),
        actions: vec![],
        entity_actions: vec![],
        read_field_renderer: FieldRendererRegistry::builder()
            .register(
                ReadProjectField::PathExists,
                project_path_status_renderer::<DynReadField>(),
            )
            .register(
                ReadProjectField::DefaultAgentModel,
                agent_model_field_renderer::<DynReadField>(Some("Codex default")),
            )
            .build(),
        create_field_renderer: FieldRendererRegistry::builder()
            .register(
                CreateProjectField::Path,
                project_path_field_renderer::<DynCreateField>(),
            )
            .register(
                CreateProjectField::DefaultAgentModel,
                agent_model_field_renderer::<DynCreateField>(None),
            )
            .register(
                CreateProjectField::DefaultAgentReasoningEffort,
                agent_reasoning_field_renderer::<DynCreateField>(None),
            )
            .build(),
        update_field_renderer: FieldRendererRegistry::builder()
            .register(
                ProjectField::Path,
                project_path_field_renderer::<DynUpdateField>(),
            )
            .register(
                ProjectField::WorkspaceMode,
                select_field_renderer::<DynUpdateField>(
                    &[
                        ("current_branch", "current_branch"),
                        ("git_worktree", "git_worktree"),
                        ("git_branch", "git_branch"),
                    ],
                    false,
                ),
            )
            .register(
                ProjectField::WorktreeCleanupPolicy,
                select_field_renderer::<DynUpdateField>(
                    &[("manual", "manual"), ("after_success", "after_success")],
                    false,
                ),
            )
            .register(
                ProjectField::DefaultAgentTool,
                select_field_renderer::<DynUpdateField>(&[("codex", "codex")], false),
            )
            .register(
                ProjectField::DefaultAgentModel,
                agent_model_field_renderer::<DynUpdateField>(Some("Codex default")),
            )
            .register(
                ProjectField::DefaultAgentReasoningEffort,
                agent_reasoning_field_renderer::<DynUpdateField>(Some("Codex default")),
            )
            .register(
                ProjectField::AgentSandboxMode,
                select_field_renderer::<DynUpdateField>(
                    &[
                        ("workspace_write", "workspace_write"),
                        ("danger_full_access", "danger_full_access"),
                    ],
                    false,
                ),
            )
            .register(
                ProjectField::AgentExtraWritableRoots,
                multiline_text_field_renderer::<DynUpdateField>(
                    "One absolute path per line; ~ is expanded on save.",
                ),
            )
            .build(),
    }
}

#[component]
fn WorkItemsPanel(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    view! {
        <section id="work-items-admin" class="work-items-admin panel">
            <div class="panel-heading">
                <h2>"Work items"</h2>
            </div>
            <div class="crudkit-work-items" data-crudkit-leptos="work-items">
                {work_items_crudkit_instance(api_base_url, project, project_id)}
            </div>
        </section>
    }
}

fn work_items_crudkit_instance(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, move |event| {
        event_scopes_named_project(event, Some(project.as_str()))
            && matches!(event, UiEvent::WorkItemChanged { .. })
    });

    view! {
        <CrudInstance
            name="work-items"
            config=work_items_crudkit_config(api_base_url, project_id)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn work_items_crudkit_config(api_base_url: String, project_id: i64) -> CrudInstanceConfig {
    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns: vec![
            Header::showing(
                ReadWorkItemField::Id,
                HeaderOptions {
                    display_name: "#".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::Title,
                HeaderOptions {
                    display_name: "Title".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::StateLabel,
                HeaderOptions {
                    display_name: "State label".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::ClaimedBy,
                HeaderOptions {
                    display_name: "Claimed by".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::Version,
                HeaderOptions {
                    display_name: "Version".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::UpdatedAt,
                HeaderOptions {
                    display_name: "Updated".into(),
                    ..Default::default()
                },
            ),
        ],
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::create_field(
                    CreateWorkItemField::Title,
                    FieldOptions {
                        label: Some(Label::new("Title")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateWorkItemField::Description,
                    FieldOptions {
                        label: Some(Label::new("Description")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateWorkItemField::AgentModelOverride,
                    FieldOptions {
                        label: Some(Label::new("Agent model override")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateWorkItemField::AgentReasoningEffortOverride,
                    FieldOptions {
                        label: Some(Label::new("Reasoning override")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    CrudWorkItem::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemField::Title,
                    FieldOptions {
                        label: Some(Label::new("Title")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemField::Description,
                    FieldOptions {
                        label: Some(Label::new("Description")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemField::AgentModelOverride,
                    FieldOptions {
                        label: Some(Label::new("Agent model override")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemField::AgentReasoningEffortOverride,
                    FieldOptions {
                        label: Some(Label::new("Reasoning override")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadWorkItem::Id.into() => Order::Desc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: Some(project_id_condition(project_id)),
        resource_name: CrudWorkItemResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: work_item_model_handler(project_id),
        actions: vec![],
        entity_actions: vec![],
        read_field_renderer: FieldRendererRegistry::builder()
            .register(
                ReadWorkItemField::AgentModelOverride,
                agent_model_field_renderer::<DynReadField>(Some("Project default")),
            )
            .build(),
        create_field_renderer: FieldRendererRegistry::builder()
            .register(
                CreateWorkItemField::AgentModelOverride,
                agent_model_field_renderer::<DynCreateField>(Some("Project default")),
            )
            .register(
                CreateWorkItemField::AgentReasoningEffortOverride,
                agent_reasoning_field_renderer::<DynCreateField>(Some("Project default")),
            )
            .build(),
        update_field_renderer: FieldRendererRegistry::builder()
            .register(
                WorkItemField::AgentModelOverride,
                agent_model_field_renderer::<DynUpdateField>(Some("Project default")),
            )
            .register(
                WorkItemField::AgentReasoningEffortOverride,
                agent_reasoning_field_renderer::<DynUpdateField>(Some("Project default")),
            )
            .build(),
    }
}

fn work_item_model_handler(project_id: i64) -> ModelHandler {
    let mut handler = ModelHandler::new::<CrudCreateWorkItem, ReadWorkItem, CrudWorkItem>();
    handler.get_default_create_model = Callback::new(move |()| {
        DynCreateModel::from(CrudCreateWorkItem {
            project_id,
            ..Default::default()
        })
    });
    handler
}

#[component]
fn SwimLanesPanel(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    view! {
        <section class="swim-lanes-admin panel">
            <div class="panel-heading">
                <h2>"Swim-lanes"</h2>
            </div>
            <div class="crudkit-swim-lanes" data-crudkit-leptos="swim-lanes">
                {swim_lanes_crudkit_instance(api_base_url, project, project_id)}
            </div>
        </section>
    }
}

fn swim_lanes_crudkit_instance(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, move |event| {
        event_scopes_named_project(event, Some(project.as_str()))
            && matches!(event, UiEvent::SwimLaneChanged { .. })
    });

    view! {
        <CrudInstance
            name="swim-lanes"
            config=swim_lanes_crudkit_config(api_base_url, project_id)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn swim_lanes_crudkit_config(api_base_url: String, project_id: i64) -> CrudInstanceConfig {
    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns: vec![
            Header::showing(
                ReadSwimLaneField::Identifier,
                HeaderOptions {
                    display_name: "Identifier".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadSwimLaneField::Name,
                HeaderOptions {
                    display_name: "Name".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadSwimLaneField::Position,
                HeaderOptions {
                    display_name: "Position".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadSwimLaneField::CanCreateItems,
                HeaderOptions {
                    display_name: "Can create items".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
        ],
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::create_field(
                    CreateSwimLaneField::Identifier,
                    FieldOptions {
                        label: Some(Label::new("Identifier")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateSwimLaneField::Name,
                    FieldOptions {
                        label: Some(Label::new("Name")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateSwimLaneField::Position,
                    FieldOptions {
                        label: Some(Label::new("Position")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateSwimLaneField::CanCreateItems,
                    FieldOptions {
                        label: Some(Label::new("Can create items")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    SwimLane::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::Identifier,
                    FieldOptions {
                        label: Some(Label::new("Identifier")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::Name,
                    FieldOptions {
                        label: Some(Label::new("Name")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::Position,
                    FieldOptions {
                        label: Some(Label::new("Position")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::CanCreateItems,
                    FieldOptions {
                        label: Some(Label::new("Can create items")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadSwimLane::Id.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: Some(project_id_condition(project_id)),
        resource_name: CrudSwimLaneResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: swim_lane_model_handler(project_id),
        actions: vec![],
        entity_actions: vec![],
        read_field_renderer: FieldRendererRegistry::builder().build(),
        create_field_renderer: FieldRendererRegistry::builder().build(),
        update_field_renderer: FieldRendererRegistry::builder().build(),
    }
}

fn swim_lane_model_handler(project_id: i64) -> ModelHandler {
    let mut handler = ModelHandler::new::<CreateSwimLane, ReadSwimLane, SwimLane>();
    handler.get_default_create_model = Callback::new(move |()| {
        DynCreateModel::from(CreateSwimLane {
            project_id,
            position: 50,
            ..Default::default()
        })
    });
    handler
}

fn agent_tools_panel(api_base_url: String) -> impl IntoView + 'static {
    view! {
        <section class="app-tools panel">
            <div class="panel-heading">
                <h2>"Codex app-server"</h2>
                <p class="muted">"Patchbay requires Codex app-server for automation."</p>
            </div>
            <form method="post" action="/agent-tools/discover">
                <input type="hidden" name="return_to" value="/projects"/>
                <button>"Check Codex"</button>
            </form>
            <div class="crudkit-agent-tools" data-crudkit-leptos="agent-tools">
                {agent_tools_crudkit_instance(api_base_url)}
            </div>
        </section>
    }
}

fn agent_tools_crudkit_instance(api_base_url: String) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, |event| {
        matches!(
            event,
            UiEvent::AgentToolChanged { .. } | UiEvent::CodexStatusChanged { .. }
        )
    });

    view! {
        <CrudInstance
            name="agent-tools"
            config=agent_tools_crudkit_config(api_base_url)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn agent_tools_crudkit_config(api_base_url: String) -> CrudInstanceConfig {
    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns: vec![
            Header::showing(
                ReadAgentToolField::Id,
                HeaderOptions {
                    display_name: "#".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadAgentToolField::ToolName,
                HeaderOptions {
                    display_name: "Tool".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadAgentToolField::ExecutablePath,
                HeaderOptions {
                    display_name: "Configured binary".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadAgentToolField::DiscoveredPath,
                HeaderOptions {
                    display_name: "Discovered binary".into(),
                    ..Default::default()
                },
            ),
        ],
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::Card(Group {
            layout: Layout::default(),
            children: vec![
                Elem::create_field(
                    CreateAgentToolField::ToolName,
                    FieldOptions {
                        label: Some(Label::new("Tool")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateAgentToolField::ExecutablePath,
                    FieldOptions {
                        label: Some(Label::new("Codex binary path")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::Card(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    AgentTool::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    AgentToolField::ExecutablePath,
                    FieldOptions {
                        label: Some(Label::new("Executable path")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadAgentTool::Id.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: None,
        resource_name: CrudAgentToolResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: ModelHandler::new::<CreateAgentTool, ReadAgentTool, AgentTool>(),
        actions: vec![],
        entity_actions: vec![],
        read_field_renderer: FieldRendererRegistry::builder().build(),
        create_field_renderer: FieldRendererRegistry::builder().build(),
        update_field_renderer: FieldRendererRegistry::builder().build(),
    }
}

#[derive(Clone, Copy)]
enum AutomationTableKind {
    Consuming,
    Producing,
}

impl AutomationTableKind {
    fn instance_name(self) -> &'static str {
        match self {
            Self::Consuming => "work-consuming-automations",
            Self::Producing => "work-producing-automations",
        }
    }

    fn effect(self) -> &'static str {
        match self {
            Self::Consuming => "consume_work",
            Self::Producing => "produce_work",
        }
    }

    fn default_activation(self) -> &'static str {
        match self {
            Self::Consuming => "work_item",
            Self::Producing => "manual",
        }
    }

    fn default_selector(self) -> Option<String> {
        match self {
            Self::Consuming => CreateAutomationTrigger::default().work_item_selector,
            Self::Producing => None,
        }
    }

    fn activation_choices(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Consuming => &[
                ("manual", "manual"),
                ("work_item", "work_item"),
                ("work_item_created", "work_item_created"),
                ("cron", "cron"),
            ],
            Self::Producing => &[("manual", "manual"), ("cron", "cron")],
        }
    }
}

fn automation_triggers_crudkit_instance(
    api_base_url: String,
    project: String,
    project_id: i64,
    kind: AutomationTableKind,
    on_context_created: Callback<CrudInstanceContext>,
) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, move |event| {
        event_scopes_named_project(event, Some(project.as_str()))
            && matches!(event, UiEvent::AutomationChanged { .. })
    });
    let created = Callback::new(move |context| {
        set_context.set(Some(context));
        on_context_created.run(context);
    });

    view! {
        <CrudInstance
            name=kind.instance_name()
            config=automation_triggers_crudkit_config(api_base_url, project_id, kind)
            on_context_created=created
        />
    }
}

fn automation_triggers_crudkit_config(
    api_base_url: String,
    project_id: i64,
    kind: AutomationTableKind,
) -> CrudInstanceConfig {
    let mut list_columns = vec![
        Header::showing(
            ReadAutomationTriggerField::Id,
            HeaderOptions {
                display_name: "#".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Name,
            HeaderOptions {
                display_name: "Name".into(),
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Activation,
            HeaderOptions {
                display_name: "Activation".into(),
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Schedule,
            HeaderOptions {
                display_name: "Schedule".into(),
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Enabled,
            HeaderOptions {
                display_name: "Enabled".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Priority,
            HeaderOptions {
                display_name: "Priority".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::EvaluationCount,
            HeaderOptions {
                display_name: "Evaluations".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::PendingEvaluationCount,
            HeaderOptions {
                display_name: "Queued".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::NextEvaluationAt,
            HeaderOptions {
                display_name: "Next evaluation".into(),
                ..Default::default()
            },
        ),
    ];
    if matches!(kind, AutomationTableKind::Consuming) {
        list_columns.insert(
            4,
            Header::showing(
                ReadAutomationTriggerField::Mode,
                HeaderOptions {
                    display_name: "Mode".into(),
                    ..Default::default()
                },
            ),
        );
    }

    let mut create_children = vec![
        Elem::create_field(
            CreateAutomationTriggerField::Name,
            FieldOptions {
                label: Some(Label::new("Name")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateAutomationTriggerField::Activation,
            FieldOptions {
                label: Some(Label::new("Activation")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateAutomationTriggerField::Schedule,
            FieldOptions {
                label: Some(Label::new("Schedule")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateAutomationTriggerField::Enabled,
            FieldOptions {
                label: Some(Label::new("Enabled")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateAutomationTriggerField::Priority,
            FieldOptions {
                label: Some(Label::new("Priority")),
                ..Default::default()
            },
        ),
    ];
    if matches!(kind, AutomationTableKind::Consuming) {
        create_children.push(Elem::create_field(
            CreateAutomationTriggerField::WorkItemSelector,
            FieldOptions {
                label: Some(Label::new("Work item selector")),
                ..Default::default()
            },
        ));
    }
    create_children.push(Elem::create_field(
        CreateAutomationTriggerField::Prompt,
        FieldOptions {
            label: Some(Label::new("Prompt")),
            ..Default::default()
        },
    ));

    let mut update_children = vec![
        Elem::field(
            AutomationTrigger::Id,
            FieldOptions {
                disabled: true,
                label: Some(Label::new("ID")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Name,
            FieldOptions {
                label: Some(Label::new("Name")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Activation,
            FieldOptions {
                label: Some(Label::new("Activation")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Schedule,
            FieldOptions {
                label: Some(Label::new("Schedule")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Enabled,
            FieldOptions {
                label: Some(Label::new("Enabled")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Priority,
            FieldOptions {
                label: Some(Label::new("Priority")),
                ..Default::default()
            },
        ),
    ];
    if matches!(kind, AutomationTableKind::Consuming) {
        update_children.push(Elem::field(
            AutomationTriggerField::WorkItemSelector,
            FieldOptions {
                label: Some(Label::new("Work item selector")),
                ..Default::default()
            },
        ));
    }
    update_children.push(Elem::field(
        AutomationTriggerField::Prompt,
        FieldOptions {
            label: Some(Label::new("Prompt")),
            ..Default::default()
        },
    ));

    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns,
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: create_children,
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: update_children,
        }))],
        order_by: indexmap! {
            ReadAutomationTrigger::Name.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: Some(automation_effect_condition(project_id, kind.effect())),
        resource_name: CrudAutomationTriggerResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: automation_trigger_model_handler(project_id, kind),
        actions: vec![],
        entity_actions: vec![],
        read_field_renderer: FieldRendererRegistry::builder().build(),
        create_field_renderer: FieldRendererRegistry::builder()
            .register(
                CreateAutomationTriggerField::Activation,
                activation_field_renderer::<DynCreateField>(kind.activation_choices()),
            )
            .build(),
        update_field_renderer: FieldRendererRegistry::builder()
            .register(
                AutomationTriggerField::Activation,
                activation_field_renderer::<DynUpdateField>(kind.activation_choices()),
            )
            .build(),
    }
}

fn automation_trigger_model_handler(project_id: i64, kind: AutomationTableKind) -> ModelHandler {
    let mut handler =
        ModelHandler::new::<CreateAutomationTrigger, ReadAutomationTrigger, AutomationTrigger>();
    handler.get_default_create_model = Callback::new(move |()| {
        DynCreateModel::from(CreateAutomationTrigger {
            project_id,
            activation: kind.default_activation().to_owned(),
            effect: kind.effect().to_owned(),
            work_item_selector: kind.default_selector(),
            ..Default::default()
        })
    });
    handler
}

fn project_path_field_renderer<F: TypeErasedField>() -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! { {move || current.get()} }.into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    view! {
                        {render_label(field_options.label.clone())}
                        <div class="project-path-field">
                            <div class="project-path-input-row">
                                <input
                                    type="text"
                                    class="crud-input-field project-path-text"
                                    prop:value=move || current.get()
                                    disabled=disabled
                                    placeholder="~/dev/project"
                                    on:input=move |event| {
                                        value_changed.run(Ok(Value::String(event_target_value(&event))));
                                    }
                                />
                                <button
                                    type="button"
                                    class="project-path-picker"
                                    disabled=disabled
                                    on:click=move |_| {
                                        let value_changed = value_changed;
                                        leptos::task::spawn_local(async move {
                                            if let Some(path) = pick_project_folder_path().await {
                                                value_changed.run(Ok(Value::String(path)));
                                            }
                                        });
                                    }
                                >
                                    "Choose folder"
                                </button>
                            </div>
                        </div>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn multiline_text_field_renderer<F: TypeErasedField>(
    placeholder: &'static str,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! { {move || current.get()} }.into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    view! {
                        {render_label(field_options.label.clone())}
                        <textarea
                            class="crud-input-field"
                            prop:value=move || current.get()
                            disabled=disabled
                            placeholder=placeholder
                            on:input=move |event| {
                                value_changed.run(Ok(Value::String(event_target_value(&event))));
                            }
                        />
                    }
                    .into_any()
                }
            }
        },
    )
}

#[cfg(not(feature = "ssr"))]
#[derive(Deserialize)]
struct PickFolderResponse {
    path: Option<String>,
}

#[cfg(not(feature = "ssr"))]
async fn pick_project_folder_path() -> Option<String> {
    let response = gloo_net::http::Request::post("/system/pick-folder")
        .send()
        .await
        .ok()?;
    if !response.ok() {
        return None;
    }
    response
        .json::<PickFolderResponse>()
        .await
        .ok()?
        .path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
}

#[cfg(feature = "ssr")]
async fn pick_project_folder_path() -> Option<String> {
    None
}

fn project_path_status_renderer<F: TypeErasedField>() -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, _field_mode, _field_options, value, _value_changed| {
            let exists = Signal::derive(move || value.value.get().as_bool().unwrap_or(false));
            view! {
                <span class=move || {
                    if exists.get() {
                        "path-status path-status-ok"
                    } else {
                        "path-status path-status-missing"
                    }
                }>
                    {move || if exists.get() { "Exists" } else { "Missing" }}
                </span>
            }
        },
    )
}

fn agent_model_field_renderer<F: TypeErasedField>(
    empty_label: Option<&'static str>,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! {
                    <span class=move || agent_model_class(&current.get())>
                        {move || agent_model_label(&current.get(), empty_label.unwrap_or("default"))}
                    </span>
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let options = CodexAgentModel::all()
                        .iter()
                        .map(|model| {
                            let value = model.as_storage();
                            view! {
                                <option value=value>{value}</option>
                            }
                        })
                        .collect::<Vec<_>>();
                    let stale_option = move || {
                        let current = current.get();
                        (!current.is_empty() && !CodexAgentModel::is_available_model(&current))
                            .then(|| {
                                let label = format!("{current} (unavailable)");
                                view! { <option value=current>{label}</option> }
                            })
                    };
                    let stale_warning = move || {
                        let current = current.get();
                        (!current.is_empty() && !CodexAgentModel::is_available_model(&current))
                            .then(|| {
                                view! {
                                    <p class="agent-model-warning">
                                        "Saved model is not available in this Codex install."
                                    </p>
                                }
                            })
                    };
                    let empty_option = empty_label.map(|empty_label| {
                        view! { <option value="">{empty_label}</option> }
                    });
                    view! {
                        {render_label(field_options.label.clone())}
                        <select
                            class="crud-input-field agent-model-select"
                            prop:value=move || current.get()
                            disabled=disabled
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if selected.trim().is_empty() {
                                    value_changed.run(Ok(Value::Null));
                                } else {
                                    value_changed.run(Ok(Value::String(selected)));
                                }
                            }
                        >
                            {empty_option}
                            {stale_option}
                            {options}
                        </select>
                        {stale_warning}
                    }
                    .into_any()
                }
            }
        },
    )
}

fn agent_model_label(value: &str, empty_label: &str) -> String {
    if value.is_empty() {
        empty_label.to_owned()
    } else if CodexAgentModel::is_available_model(value) {
        value.to_owned()
    } else {
        format!("{value} (unavailable)")
    }
}

fn agent_model_class(value: &str) -> &'static str {
    if value.is_empty() {
        "agent-model-value agent-model-default"
    } else if CodexAgentModel::is_available_model(value) {
        "agent-model-value"
    } else {
        "agent-model-value agent-model-stale"
    }
}

fn agent_reasoning_field_renderer<F: TypeErasedField>(
    empty_label: Option<&'static str>,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! {
                    {move || {
                        let current = current.get();
                        if current.is_empty() {
                            empty_label.unwrap_or("default").to_owned()
                        } else {
                            current
                        }
                    }}
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let options = AgentReasoningEffort::all()
                        .into_iter()
                        .map(|effort| {
                            let value = effort.as_storage();
                            view! {
                                <option value=value>{effort.to_string()}</option>
                            }
                        })
                        .collect::<Vec<_>>();
                    let empty_option = empty_label.map(|empty_label| {
                        view! { <option value="">{empty_label}</option> }
                    });
                    view! {
                        {render_label(field_options.label.clone())}
                        <select
                            class="crud-input-field agent-reasoning-select"
                            prop:value=move || current.get()
                            disabled=disabled
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if selected.trim().is_empty() {
                                    value_changed.run(Ok(Value::Null));
                                } else {
                                    value_changed.run(Ok(Value::String(selected)));
                                }
                            }
                        >
                            {empty_option}
                            {options}
                        </select>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn activation_field_renderer<F: TypeErasedField>(
    choices: &'static [(&'static str, &'static str)],
) -> FieldRenderer<F> {
    select_field_renderer(choices, false)
}

fn select_field_renderer<F: TypeErasedField>(
    choices: &'static [(&'static str, &'static str)],
    nullable: bool,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! {
                    {move || {
                        let current = current.get();
                        choices
                            .iter()
                            .find(|(value, _)| *value == current)
                            .map(|(_, label)| (*label).to_owned())
                            .unwrap_or(current)
                    }}
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let options = choices
                        .iter()
                        .map(|(value, label)| {
                            view! {
                                <option value=*value>{*label}</option>
                            }
                        })
                        .collect::<Vec<_>>();
                    let empty_option = nullable.then(|| {
                        view! { <option value="">"default"</option> }
                    });
                    view! {
                        {render_label(field_options.label.clone())}
                        <select
                            class="crud-input-field"
                            prop:value=move || current.get()
                            disabled=disabled
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if nullable && selected.trim().is_empty() {
                                    value_changed.run(Ok(Value::Null));
                                } else {
                                    value_changed.run(Ok(Value::String(selected)));
                                }
                            }
                        >
                            {empty_option}
                            {options}
                        </select>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn project_id_condition(project_id: i64) -> Condition {
    Condition::All(vec![ConditionElement::Clause(ConditionClause {
        column_name: "project_id".to_owned(),
        operator: Operator::Equal,
        value: ConditionClauseValue::I64(project_id),
    })])
}

fn automation_effect_condition(project_id: i64, effect: &str) -> Condition {
    Condition::All(vec![
        ConditionElement::Clause(ConditionClause {
            column_name: "project_id".to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::I64(project_id),
        }),
        ConditionElement::Clause(ConditionClause {
            column_name: "effect".to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::String(effect.to_owned()),
        }),
    ])
}

fn project_workspace_panel(
    project: &str,
    project_view: &ProjectView,
    return_to: String,
) -> AnyView {
    workspace_actions(
        "Path",
        project_view.path.clone(),
        Some(project_view.path_exists),
        Some(format!("/projects/{}/workspace/open", encode_path(project))),
        return_to,
    )
}

fn run_workspace_actions(project: &str, run: &AgentRunView, return_to: String) -> AnyView {
    workspace_actions(
        "working dir",
        non_empty_string(run.working_dir.clone()),
        None,
        Some(format!(
            "/projects/{}/automation/runs/{}/workspace/open",
            encode_path(project),
            run.id
        )),
        return_to,
    )
}

fn workspace_actions(
    label: &'static str,
    path: Option<String>,
    path_exists: Option<bool>,
    open_action: Option<String>,
    return_to: String,
) -> AnyView {
    let path = path.and_then(non_empty_string);
    let copy_available = path.is_some();
    let open_available = copy_available && path_exists.unwrap_or(true);
    let display_path = path.clone().unwrap_or_else(|| "not configured".to_owned());
    let copy_path = path.clone().unwrap_or_default();
    let copy_cd = path.as_deref().map(shell_cd_command).unwrap_or_default();
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
    let open_controls = open_action.map(|action| {
        let folder_action = action.clone();
        let folder_return = return_to.clone();
        let ide_return = return_to.clone();
        view! {
            <>
                <form method="post" action=folder_action>
                    <input type="hidden" name="target" value="folder"/>
                    <input type="hidden" name="return_to" value=folder_return/>
                    <button type="submit" class="secondary workspace-button" disabled=!open_available>
                        "Open folder"
                    </button>
                </form>
                <form method="post" action=action>
                    <input type="hidden" name="target" value="ide"/>
                    <input type="hidden" name="return_to" value=ide_return/>
                    <button type="submit" class="secondary workspace-button" disabled=!open_available>
                        "Open IDE"
                    </button>
                </form>
            </>
        }
    });
    let path_for_copy = copy_path.clone();
    let cd_for_copy = copy_cd.clone();

    view! {
        <div class="workspace-actions">
            <div class="workspace-path">
                <span class="workspace-label">{label}</span>
                <code>{display_path}</code>
                {status}
            </div>
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
                <button
                    type="button"
                    class="secondary workspace-button"
                    disabled=!copy_available
                    on:click=move |_| {
                        copy_workspace_text(
                            cd_for_copy.clone(),
                            "Copied cd",
                            set_copy_message,
                        );
                    }
                >
                    "Copy cd"
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

fn shell_cd_command(path: &str) -> String {
    format!("cd {}", shell_quote(path))
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
    set_create_item_swim_lanes: WriteSignal<Vec<SwimLaneView>>,
) -> impl IntoView + 'static {
    let (items, set_items) = signal(initial_items);
    let (swim_lanes, set_swim_lanes) = signal(initial_swim_lanes);
    let (misconfigured_item_count, set_misconfigured_item_count) =
        signal(initial_misconfigured_item_count);
    let project_for_loader = project.clone();
    let section = LocalResource::new(move || load_board_items_section(project_for_loader.clone()));
    let _poll = use_interval_fn(move || section.refetch(), BOARD_ITEMS_REFRESH_INTERVAL_MS);
    let project_for_events = project.clone();
    refetch_on_live_event(section, move |event| {
        event_scopes_named_project(event, Some(project_for_events.as_str()))
            && matches!(
                event,
                UiEvent::WorkItemChanged { .. } | UiEvent::SwimLaneChanged { .. }
            )
    });

    Effect::new(move |_| {
        if let Some(Ok(section)) = section.get() {
            set_items.set(section.items);
            let updated_swim_lanes = section.swim_lanes;
            set_create_item_swim_lanes.set(updated_swim_lanes.clone());
            set_swim_lanes.set(updated_swim_lanes);
            set_misconfigured_item_count.set(section.misconfigured_item_count);
        }
    });

    view! {
        {move || {
            board_view(
                project.clone(),
                items.get(),
                swim_lanes.get(),
                misconfigured_item_count.get(),
                open_create_item,
            )
        }}
    }
}

#[component]
fn LiveBoardAutomation(
    project: String,
    initial_status: AutomationStatusView,
    initial_running: bool,
    initial_run_sessions: Vec<BoardRunSessionView>,
) -> impl IntoView + 'static {
    let (automation_status, set_automation_status) = signal(initial_status);
    let (automation_running, set_automation_running) = signal(initial_running);
    let (run_sessions, set_run_sessions) = signal(initial_run_sessions);
    let project_for_loader = project.clone();
    let section =
        LocalResource::new(move || load_board_automation_section(project_for_loader.clone()));
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
        let controller = if automation_running.get() {
            "controller running"
        } else {
            "controller stopped"
        };
        Some(format!("{running_runs} running, {controller}"))
    });

    view! {
        <RunSessionsPanel
            project=project
            title="Runs"
            status_note=status_note
            run_sessions=run_sessions
            empty_message="No runs yet."
        />
    }
}

fn project_settings_view(
    project: &str,
    project_view: ProjectView,
    _settings: ProjectSettingsView,
    memory_events: Vec<ProjectMemoryEventView>,
) -> impl IntoView + 'static {
    let prompt_action = format!("/projects/{}/system-prompt", encode_path(project));
    let memory_action = format!("/projects/{}/memory", encode_path(project));
    let initial_memory = project_view.memory.clone();
    let dirty_baseline = initial_memory.clone();
    let history_for_options = memory_events.clone();
    let history_for_memory = memory_events.clone();
    let (selected_event_id, set_selected_event_id) = signal(None::<i64>);
    let (memory_draft, set_memory_draft) = signal(initial_memory.clone());
    let memory_value = move || {
        selected_event_id
            .get()
            .and_then(|event_id| {
                history_for_memory
                    .iter()
                    .find(|event| event.id == event_id)
                    .map(|event| event.memory.clone())
                    .or_else(|| Some(format!("Memory event #{event_id} is no longer available.")))
            })
            .unwrap_or_else(|| memory_draft.get())
    };
    let memory_textarea_class = move || {
        if selected_event_id.get().is_none() && memory_draft.get() != dirty_baseline {
            "project-memory-text dirty"
        } else {
            "project-memory-text"
        }
    };
    let event_options = history_for_options
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
                    <textarea name="body" placeholder="Project system prompt">
                        {project_view.system_prompt}
                    </textarea>
                    <button>"Save prompt"</button>
                </form>
            </div>
            <div>
                <h2>"Memory"</h2>
                <form method="post" action=memory_action>
                    <div class="memory-history">
                        <label for="project-memory-version">"memory history"</label>
                        <select
                            id="project-memory-version"
                            prop:value=move || {
                                selected_event_id
                                    .get()
                                    .map(|event_id| event_id.to_string())
                                    .unwrap_or_else(|| "current".to_owned())
                            }
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if selected == "current" {
                                    set_selected_event_id.set(None);
                                } else if let Ok(event_id) = selected.parse::<i64>() {
                                    set_selected_event_id.set(Some(event_id));
                                }
                            }
                        >
                            <option value="current">"Current"</option>
                            {event_options}
                        </select>
                    </div>
                    <textarea
                        name="body"
                        class=memory_textarea_class
                        placeholder="Project memory"
                        prop:value=memory_value
                        readonly=move || selected_event_id.get().is_some()
                        on:input=move |event| {
                            if selected_event_id.get().is_none() {
                                set_memory_draft.set(event_target_value(&event));
                            }
                        }
                    >
                        {initial_memory}
                    </textarea>
                    <button disabled=move || selected_event_id.get().is_some()>"Save memory"</button>
                </form>
            </div>
        </section>
    }
}

fn memory_event_select_label(event: &ProjectMemoryEventView) -> String {
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
    empty_message: &'static str,
) -> impl IntoView + 'static {
    let (selected_run_id, set_selected_run_id) = signal(None::<i64>);
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
                        on:click=move |_| set_selected_run_id.set(Some(run_id))
                    >
                        <div class="session-head">
                            <strong>"#" {run_id}</strong>
                            <span>{session.run.status.to_string()}</span>
                            <span>{session.run.mode.to_string()}</span>
                            {origin.map(|origin| view! { <span>{origin}</span> })}
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
            Some(session) => run_session_detail(&detail_project, session),
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

fn recorded_field(value: &str) -> String {
    if value.trim().is_empty() {
        "not recorded".to_owned()
    } else {
        value.to_owned()
    }
}

fn run_session_detail(project: &str, session: BoardRunSessionView) -> AnyView {
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
    let summary = run_result_summary(&session.run);
    let origin = run_origin_label(&session.run);
    let command = recorded_field(&session.run.command);
    let working_dir = run_workspace_actions(project, &session.run, href.clone());
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
                        {session.run.mode.to_string()}
                        " · cleanup "
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
                <dt>"model"</dt>
                <dd>{model}</dd>
                <dt>"reasoning"</dt>
                <dd>{reasoning}</dd>
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

fn create_item_options_for_request(
    swim_lanes: &[SwimLaneView],
    request: &CreateItemOpenRequest,
) -> Vec<CreateItemLaneOption> {
    match request {
        CreateItemOpenRequest::AnyCreatableLane => creatable_lane_options(swim_lanes),
        CreateItemOpenRequest::SingleLane(identifier) => swim_lanes
            .iter()
            .filter(|lane| lane.can_create_items && lane.identifier == *identifier)
            .map(create_item_lane_option)
            .collect(),
    }
}

fn creatable_lane_options(swim_lanes: &[SwimLaneView]) -> Vec<CreateItemLaneOption> {
    swim_lanes
        .iter()
        .filter(|lane| lane.can_create_items)
        .map(create_item_lane_option)
        .collect()
}

fn create_item_lane_option(lane: &SwimLaneView) -> CreateItemLaneOption {
    CreateItemLaneOption {
        identifier: lane.identifier.clone(),
        name: lane.name.clone(),
    }
}

fn default_create_item_state(options: &[CreateItemLaneOption]) -> String {
    options
        .iter()
        .find(|option| option.identifier == DEFAULT_CREATE_ITEM_STATE)
        .or_else(|| options.first())
        .map(|option| option.identifier.clone())
        .unwrap_or_else(|| DEFAULT_CREATE_ITEM_STATE.to_owned())
}

fn create_item_lane_option_views(
    options: Vec<CreateItemLaneOption>,
    selected_state: String,
) -> Vec<AnyView> {
    if options.is_empty() {
        return vec![
            view! {
                <option value="" selected=true>"No lanes available"</option>
            }
            .into_any(),
        ];
    }

    options
        .into_iter()
        .map(|option| {
            let selected = option.identifier == selected_state;
            view! {
                <option value=option.identifier selected=selected>
                    {option.name}
                </option>
            }
            .into_any()
        })
        .collect()
}

fn create_item_modal(
    project: &str,
    show_when: ReadSignal<bool>,
    set_show_when: WriteSignal<bool>,
    lane_options: ReadSignal<Vec<CreateItemLaneOption>>,
    selected_state: ReadSignal<String>,
    set_selected_state: WriteSignal<String>,
) -> impl IntoView + 'static {
    let action = StoredValue::new(format!("/projects/{}/items", encode_path(project)));
    view! {
        <Modal
            id="new-item-modal"
            class="new-item-modal"
            show_when=show_when
            on_escape=move || set_show_when.set(false)
            on_backdrop_interaction=move || set_show_when.set(false)
        >
            <form class="new-item-form" method="post" action=move || action.get_value()>
                <ModalHeader>
                    <ModalTitle>"New item"</ModalTitle>
                    <button
                        type="button"
                        class="secondary"
                        on:click=move |_| set_show_when.set(false)
                    >
                        "Close"
                    </button>
                </ModalHeader>
                <ModalBody>
                    <label>
                        <span>"Title"</span>
                        <input name="title" placeholder="Title" required/>
                    </label>
                    <label>
                        <span>"Description"</span>
                        <textarea name="description" placeholder="Description" required></textarea>
                    </label>
                    <label>
                        <span>"Lane"</span>
                        <select
                            name="state"
                            prop:value=move || selected_state.get()
                            disabled=move || lane_options.get().is_empty()
                            on:change=move |event| {
                                set_selected_state.set(event_target_value(&event));
                            }
                        >
                            {move || create_item_lane_option_views(
                                lane_options.get(),
                                selected_state.get(),
                            )}
                        </select>
                    </label>
                    <label>
                        <span>"Agent model override"</span>
                        <select name="agent_model_override">
                            {agent_model_options(None, "Project default")}
                        </select>
                    </label>
                    <label>
                        <span>"Reasoning override"</span>
                        <select name="agent_reasoning_effort_override">
                            {agent_reasoning_options(None, "Project default")}
                        </select>
                    </label>
                </ModalBody>
                <ModalFooter>
                    <button
                        type="button"
                        class="secondary"
                        on:click=move |_| set_show_when.set(false)
                    >
                        "Cancel"
                    </button>
                    <button type="submit" disabled=move || lane_options.get().is_empty()>
                        "Create item"
                    </button>
                </ModalFooter>
            </form>
        </Modal>
    }
}

fn agent_model_options(selected: Option<String>, empty_label: &'static str) -> Vec<AnyView> {
    let selected = selected
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let mut options = vec![
        view! {
            <option value="" selected=selected.is_none()>{empty_label}</option>
        }
        .into_any(),
    ];
    if let Some(value) = selected
        .as_ref()
        .filter(|value| !CodexAgentModel::is_available_model(value))
    {
        let option_value = value.clone();
        let label = format!("{value} (unavailable)");
        options.push(
            view! {
                <option value=option_value selected=true>{label}</option>
            }
            .into_any(),
        );
    }
    options.extend(CodexAgentModel::all().into_iter().map(|model| {
        let value = model.as_storage();
        view! {
            <option value=value selected=selected.as_deref() == Some(value)>
                {value}
            </option>
        }
        .into_any()
    }));
    options
}

fn agent_reasoning_options(
    selected: Option<AgentReasoningEffort>,
    empty_label: &'static str,
) -> Vec<AnyView> {
    let mut options = vec![
        view! {
            <option value="" selected=selected.is_none()>{empty_label}</option>
        }
        .into_any(),
    ];
    options.extend(AgentReasoningEffort::all().into_iter().map(|effort| {
        view! {
            <option value=effort.as_storage() selected=selected == Some(effort)>
                {effort.to_string()}
            </option>
        }
        .into_any()
    }));
    options
}

fn board_view(
    project: String,
    items: Vec<WorkItemView>,
    swim_lanes: Vec<SwimLaneView>,
    misconfigured_item_count: i64,
    open_create_item: Callback<CreateItemOpenRequest>,
) -> impl IntoView + 'static {
    let lanes = swim_lanes
        .into_iter()
        .map(|lane| {
            let lane_identifier = lane.identifier.clone();
            let label = lane.name;
            let cards = items
                .iter()
                .filter(|item| item.state.as_deref() == Some(lane_identifier.as_str()))
                .cloned()
                .map(|item| item_card(project.clone(), item))
                .collect::<Vec<_>>();
            let count = cards.len();
            let create_state = lane_identifier.clone();
            let add_button = if lane.can_create_items {
                view! {
                    <button
                        type="button"
                        class="lane-add"
                        on:click=move |_| {
                            open_create_item.run(CreateItemOpenRequest::SingleLane(create_state.clone()))
                        }
                    >
                        "+ Add"
                    </button>
                }
                .into_any()
            } else {
                ().into_any()
            };
            view! {
                <section class="lane">
                    <header class="lane-header">
                        <h2>{label}</h2>
                        <span class="lane-count">{count}</span>
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
            "is"
        } else {
            "are"
        };
        let message = format!(
            "{misconfigured_item_count} {item_word} {verb} incorrectly labeled or unlabeled."
        );

        view! {
            <section class="board-state-warning" role="status">
                <strong>"Swim-lane warning"</strong>
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

fn item_card(project: String, item: WorkItemView) -> impl IntoView + 'static {
    let href = format!("/projects/{}/items/{}", encode_path(&project), item.id);
    let description = preview(&item.description);
    let claimed = item.claimed_by.is_some();
    let label_chips = item
        .labels
        .iter()
        .map(|label| {
            let blocked = label.key == AUTOMATION_BLOCKED_LABEL_KEY;
            let label = format_label(&label.key, label.value.as_deref());
            view! { <span class="label-chip" class:blocked=blocked>{label}</span> }
        })
        .collect::<Vec<_>>();
    let claim = item.claimed_by.clone().map(|agent| {
        let status = if item.state.as_deref() == Some("in_progress") {
            "In progress"
        } else {
            "Claimed"
        };
        view! {
            <div class="claim-badge">
                <span class="claim-dot" aria-hidden="true"></span>
                <span>{status}</span>
                <span class="claim-agent">{agent}</span>
            </div>
        }
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
    let codex_href = format!("/codex{selected_query}");
    let projects_href = format!("/projects{selected_query}");
    let api_href = format!("/api/docs{selected_query}");
    let board_class = active_class(active, ActivePage::Board);
    let triggers_class = active_class(active, ActivePage::Triggers);
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
    if control.running {
        let stop_action = format!(
            "/projects/{}/automation/stop",
            encode_path(&control.project)
        );
        view! {
            <form class="topbar-automation" method="post" action=stop_action>
                <span class="automation-status running">"Running"</span>
                <button type="submit" class="danger">"Stop"</button>
            </form>
        }
        .into_any()
    } else {
        let start_action = format!(
            "/projects/{}/automation/start",
            encode_path(&control.project)
        );
        view! {
            <form class="topbar-automation" method="post" action=start_action>
                <input type="hidden" name="mode" value="execute"/>
                <span class="automation-status stopped">"Stopped"</span>
                <button type="submit">"Start"</button>
            </form>
        }
        .into_any()
    }
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

fn state_label(item: &WorkItemView) -> &str {
    item.state.as_deref().unwrap_or("(no state)")
}

fn format_label(key: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("{key}={value}"),
        None => key.to_owned(),
    }
}

fn preview(value: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 140;
    if value.chars().count() <= MAX_PREVIEW_CHARS {
        return value.to_owned();
    }

    value.chars().take(MAX_PREVIEW_CHARS).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::infer_agent_comment_run_id;

    #[test]
    fn infers_run_id_from_patchbay_agent_name() {
        assert_eq!(infer_agent_comment_run_id("patchbay-run-60"), Some(60));
    }

    #[test]
    fn ignores_non_run_agent_names() {
        assert_eq!(infer_agent_comment_run_id("codex"), None);
        assert_eq!(infer_agent_comment_run_id("patchbay-run-"), None);
        assert_eq!(infer_agent_comment_run_id("patchbay-run-0"), None);
        assert_eq!(infer_agent_comment_run_id("patchbay-run-+60"), None);
        assert_eq!(infer_agent_comment_run_id("patchbay-run-abc"), None);
    }
}
