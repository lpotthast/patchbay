use std::collections::{HashMap, HashSet};

use rootcause::Result;

use crate::{
    backend::{
        automation, automation_controller::AutomationController, codex_app_server, comments, items,
        process_sessions::ProcessSessionRegistry, projects, storage::Store, swim_lanes,
    },
    frontend::{
        ApiDocsPage, BoardAutomationSection, BoardItemsSection, BoardPage, BoardRunSessionView,
        CodexStatusPage, ItemPage, ProjectsPage, RunLogPage, RuntimeConfigView, TriggersPage,
    },
    shared::view_models::{AgentRunView, CodexAppServerStatusView, ProcessSessionView},
};

pub(crate) async fn board_page_data(
    store: &Store,
    sessions: &ProcessSessionRegistry,
    automation_controller: &AutomationController,
    codex_status: CodexAppServerStatusView,
    selected_project: Option<&str>,
    api_base_url: String,
) -> Result<BoardPage> {
    let projects = projects::list_projects(store).await?;
    let active_project_names = active_project_names(store, automation_controller).await?;
    let selected_project = selected_project
        .or_else(|| projects.first().map(|project| project.name.as_str()))
        .map(ToOwned::to_owned);

    let selected_project_view = selected_project
        .as_deref()
        .and_then(|project| projects.iter().find(|candidate| candidate.name == project))
        .cloned();

    let mut settings = None;
    let mut memory_events = Vec::new();
    let mut automation_status = None;
    let mut automation_running = false;
    let mut run_sessions = Vec::new();
    let mut project_items = Vec::new();
    let mut project_swim_lanes = Vec::new();
    let mut misconfigured_item_count = 0;
    if let Some(project) = selected_project_view
        .as_ref()
        .map(|project| project.name.as_str())
    {
        settings = Some(projects::get_settings(store, project).await?);
        memory_events = projects::list_memory_events(store, project).await?;
        let status = automation::automation_status(store, project).await?;
        automation_running = automation_controller.is_project_running(project).await;
        let active_sessions = sessions.list_for_project(project).await;
        run_sessions =
            board_run_sessions(store, project, status.recent_runs.clone(), active_sessions).await?;
        automation_status = Some(status);
        project_items = items::list_items(store, project, None).await?;
        project_swim_lanes = swim_lanes::list_swim_lanes(store, project).await?;
        misconfigured_item_count = items::count_items_outside_swim_lanes(store, project).await?;
    }

    Ok(BoardPage {
        projects,
        active_project_names,
        selected_project,
        selected_project_view,
        settings,
        memory_events,
        automation_status,
        automation_running,
        run_sessions,
        items: project_items,
        swim_lanes: project_swim_lanes,
        misconfigured_item_count,
        api_base_url,
        codex_status,
        runtime: runtime_config_view(store),
    })
}

pub(crate) async fn board_items_section(store: &Store, project: &str) -> Result<BoardItemsSection> {
    Ok(BoardItemsSection {
        items: items::list_items(store, project, None).await?,
        swim_lanes: swim_lanes::list_swim_lanes(store, project).await?,
        misconfigured_item_count: items::count_items_outside_swim_lanes(store, project).await?,
    })
}

pub(crate) async fn board_automation_section(
    store: &Store,
    sessions: &ProcessSessionRegistry,
    automation_controller: &AutomationController,
    project: &str,
) -> Result<BoardAutomationSection> {
    let automation_status = automation::automation_status(store, project).await?;
    let automation_running = automation_controller.is_project_running(project).await;
    let active_sessions = sessions.list_for_project(project).await;
    let run_sessions = board_run_sessions(
        store,
        project,
        automation_status.recent_runs.clone(),
        active_sessions,
    )
    .await?;

    Ok(BoardAutomationSection {
        automation_status,
        automation_running,
        run_sessions,
    })
}

fn runtime_config_view(store: &Store) -> RuntimeConfigView {
    RuntimeConfigView {
        database_path: store.path().to_string_lossy().into_owned(),
        database_directory: store
            .path()
            .parent()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        codex_home_path: codex_app_server::codex_home_dir()
            .to_string_lossy()
            .into_owned(),
        codex_config_path: codex_app_server::codex_config_path()
            .to_string_lossy()
            .into_owned(),
    }
}

async fn board_run_sessions(
    store: &Store,
    project: &str,
    recent_runs: Vec<AgentRunView>,
    active_sessions: Vec<ProcessSessionView>,
) -> Result<Vec<BoardRunSessionView>> {
    let active_by_run = active_sessions
        .into_iter()
        .map(|session| (session.run_id, session))
        .collect::<HashMap<_, _>>();
    let active_ids = active_by_run.keys().copied().collect::<HashSet<_>>();
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();

    for run in recent_runs
        .iter()
        .filter(|run| active_ids.contains(&run.id))
        .cloned()
    {
        seen.insert(run.id);
        ordered.push(run);
    }
    let missing_active_ids = active_ids
        .iter()
        .copied()
        .filter(|run_id| !seen.contains(run_id))
        .collect::<Vec<_>>();
    for run_id in missing_active_ids {
        let run = automation::get_run(store, project, run_id).await?;
        seen.insert(run.id);
        ordered.push(run);
    }
    for run in recent_runs
        .into_iter()
        .filter(|run| !seen.contains(&run.id))
    {
        ordered.push(run);
    }

    let mut views = Vec::with_capacity(ordered.len());
    for run in ordered {
        let run_id = run.id;
        let active_session = active_by_run.get(&run_id);
        let run_log = automation::read_run_log(store, project, run_id).await?;
        let output = active_session
            .filter(|session| !session.output.is_empty())
            .map(|session| session.output.clone())
            .unwrap_or(run_log.output);
        views.push(BoardRunSessionView {
            run,
            prompt: run_log.prompt,
            output,
            active: active_session.is_some(),
        });
    }
    Ok(views)
}

pub(crate) async fn trigger_run_sessions(
    store: &Store,
    sessions: &ProcessSessionRegistry,
    project: &str,
    trigger_id: i64,
) -> Result<Vec<BoardRunSessionView>> {
    let runs = automation::list_runs_for_trigger(store, project, trigger_id, None).await?;
    let run_ids = runs.iter().map(|run| run.id).collect::<HashSet<_>>();
    let active_sessions = sessions
        .list_for_project(project)
        .await
        .into_iter()
        .filter(|session| run_ids.contains(&session.run_id))
        .collect::<Vec<_>>();
    board_run_sessions(store, project, runs, active_sessions).await
}

pub(crate) async fn item_page_data(
    store: &Store,
    automation_controller: &AutomationController,
    project: &str,
    item_id: i64,
    codex_status: CodexAppServerStatusView,
) -> Result<ItemPage> {
    let projects = projects::list_projects(store).await?;
    let active_project_names = active_project_names(store, automation_controller).await?;
    let item = items::get_item(store, project, item_id).await?;
    let comments = comments::list_comments(store, project, item_id).await?;
    let label_suggestions = items::list_project_labels(store, project).await?;
    let automation_runs = automation::list_runs_for_item(store, project, item_id, Some(10)).await?;
    Ok(ItemPage {
        projects,
        active_project_names,
        project: project.to_owned(),
        item,
        comments,
        label_suggestions,
        automation_runs,
        codex_status,
    })
}

pub(crate) async fn run_log_page_data(
    store: &Store,
    automation_controller: &AutomationController,
    project: &str,
    run_id: i64,
    codex_status: CodexAppServerStatusView,
) -> Result<RunLogPage> {
    let projects = projects::list_projects(store).await?;
    let active_project_names = active_project_names(store, automation_controller).await?;
    let run_log = automation::read_run_log(store, project, run_id).await?;
    Ok(RunLogPage {
        projects,
        active_project_names,
        project: project.to_owned(),
        run_log,
        codex_status,
    })
}

pub(crate) async fn projects_page_data(
    store: &Store,
    automation_controller: &AutomationController,
    codex_status: CodexAppServerStatusView,
    selected_project: Option<&str>,
    api_base_url: String,
) -> Result<ProjectsPage> {
    let projects = projects::list_projects(store).await?;
    let active_project_names = active_project_names(store, automation_controller).await?;
    let selected_project = selected_project
        .or_else(|| projects.first().map(|project| project.name.as_str()))
        .map(ToOwned::to_owned);

    Ok(ProjectsPage {
        projects,
        active_project_names,
        selected_project,
        api_base_url,
        codex_status,
    })
}

pub(crate) async fn triggers_page_data(
    store: &Store,
    automation_controller: &AutomationController,
    codex_status: CodexAppServerStatusView,
    selected_project: Option<&str>,
    api_base_url: String,
) -> Result<TriggersPage> {
    let projects = projects::list_projects(store).await?;
    let active_project_names = active_project_names(store, automation_controller).await?;
    let selected_project = selected_project
        .or_else(|| projects.first().map(|project| project.name.as_str()))
        .map(ToOwned::to_owned);
    let selected_project_view = selected_project
        .as_deref()
        .and_then(|project| projects.iter().find(|candidate| candidate.name == project))
        .cloned();

    Ok(TriggersPage {
        projects,
        active_project_names,
        selected_project,
        selected_project_view,
        api_base_url,
        codex_status,
    })
}

pub(crate) async fn codex_status_page_data(
    store: &Store,
    automation_controller: &AutomationController,
    codex_status: CodexAppServerStatusView,
    selected_project: Option<&str>,
) -> Result<CodexStatusPage> {
    let projects = projects::list_projects(store).await?;
    let active_project_names = active_project_names(store, automation_controller).await?;
    let selected_project = selected_project
        .or_else(|| projects.first().map(|project| project.name.as_str()))
        .map(ToOwned::to_owned);

    Ok(CodexStatusPage {
        projects,
        active_project_names,
        selected_project,
        codex_status,
    })
}

pub(crate) async fn api_docs_page_data(
    store: &Store,
    automation_controller: &AutomationController,
    codex_status: CodexAppServerStatusView,
    selected_project: Option<&str>,
) -> Result<ApiDocsPage> {
    let projects = projects::list_projects(store).await?;
    let active_project_names = active_project_names(store, automation_controller).await?;
    let selected_project = selected_project
        .or_else(|| projects.first().map(|project| project.name.as_str()))
        .map(ToOwned::to_owned);

    Ok(ApiDocsPage {
        projects,
        active_project_names,
        selected_project,
        codex_status,
    })
}

async fn active_project_names(
    store: &Store,
    automation_controller: &AutomationController,
) -> Result<Vec<String>> {
    let mut active = automation::active_project_names(store).await?;
    active.extend(automation_controller.active_project_names().await);
    active.sort();
    active.dedup();
    Ok(active)
}
