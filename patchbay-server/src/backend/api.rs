use anyhow::{Result, bail};
use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use patchbay_types::{
    AddCommentRequest, ApiError, ClaimWorkItemRequest, ClaimWorkItemResponse,
    CreateWorkItemRequest, FinishWorkItemRequest, ProgressWorkItemRequest, ReleaseWorkItemRequest,
    UpdateProjectMemoryRequest, UpdateWorkItemRequest, WorkState,
};
use serde::{Deserialize, Serialize};

use crate::backend::{
    automation, comments,
    comments::AddComment,
    items,
    items::{CreateWorkItem, UpdateWorkItem},
    projects,
    ui::AppState,
};

#[derive(Debug, Deserialize)]
pub(crate) struct ListItemsQuery {
    state: Option<WorkState>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListRunsQuery {
    limit: Option<u64>,
}

pub(crate) async fn get_project(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    json_result(projects::get_project(&state.store, &project).await)
}

pub(crate) async fn get_project_settings(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    json_result(projects::get_settings(&state.store, &project).await)
}

pub(crate) async fn get_project_memory(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    json_result(projects::get_memory(&state.store, &project).await)
}

pub(crate) async fn list_project_memory_events(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    json_result(projects::list_memory_events(&state.store, &project).await)
}

pub(crate) async fn set_project_memory(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Json(request): Json<UpdateProjectMemoryRequest>,
) -> Response {
    json_result(
        projects::update_memory_with_source(
            &state.store,
            &project,
            request.body,
            projects::MemoryChangeSource::Agent {
                agent_id: request.agent_id,
                agent_run_id: request.agent_run_id,
            },
        )
        .await,
    )
}

pub(crate) async fn append_project_memory(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Json(request): Json<UpdateProjectMemoryRequest>,
) -> Response {
    json_result(
        projects::append_memory_with_source(
            &state.store,
            &project,
            request.body,
            projects::MemoryChangeSource::Agent {
                agent_id: request.agent_id,
                agent_run_id: request.agent_run_id,
            },
        )
        .await,
    )
}

pub(crate) async fn compact_project_memory_events(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    json_result(projects::compact_memory_events(&state.store, &project).await)
}

pub(crate) async fn list_items(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Query(query): Query<ListItemsQuery>,
) -> Response {
    json_result(items::list_items(&state.store, &project, query.state).await)
}

pub(crate) async fn create_item(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Json(request): Json<CreateWorkItemRequest>,
) -> Response {
    json_result(
        items::create_item(
            &state.store,
            &project,
            CreateWorkItem {
                title: request.title,
                description: request.description,
                automation_claimable: request.automation_claimable,
                agent_model_override: request.agent_model_override,
                agent_reasoning_effort_override: request.agent_reasoning_effort_override,
            },
        )
        .await,
    )
}

pub(crate) async fn get_item(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
) -> Response {
    json_result(items::get_item(&state.store, &project, item_id).await)
}

pub(crate) async fn update_item(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Json(request): Json<UpdateWorkItemRequest>,
) -> Response {
    json_result(update_item_inner(&state, &project, item_id, request).await)
}

pub(crate) async fn claim_item(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Json(request): Json<ClaimWorkItemRequest>,
) -> Response {
    json_result(
        items::claim_item(&state.store, &project, &request.agent_id, request.state)
            .await
            .map(|item| ClaimWorkItemResponse { item }),
    )
}

pub(crate) async fn progress_item(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Json(request): Json<ProgressWorkItemRequest>,
) -> Response {
    json_result(
        items::progress_item(
            &state.store,
            &project,
            item_id,
            &request.agent_id,
            &request.body,
        )
        .await,
    )
}

pub(crate) async fn finish_item(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Json(request): Json<FinishWorkItemRequest>,
) -> Response {
    json_result(
        items::finish_item(
            &state.store,
            &project,
            item_id,
            &request.agent_id,
            &request.report,
        )
        .await,
    )
}

pub(crate) async fn release_item(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Json(request): Json<ReleaseWorkItemRequest>,
) -> Response {
    json_result(
        items::release_item(
            &state.store,
            &project,
            item_id,
            &request.agent_id,
            request.comment,
        )
        .await,
    )
}

pub(crate) async fn list_comments(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
) -> Response {
    json_result(comments::list_comments(&state.store, &project, item_id).await)
}

pub(crate) async fn add_comment(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Json(request): Json<AddCommentRequest>,
) -> Response {
    json_result(
        comments::add_comment(
            &state.store,
            &project,
            item_id,
            AddComment {
                author_type: request.author_type,
                author_name: request.author_name,
                body: request.body,
            },
        )
        .await,
    )
}

pub(crate) async fn list_runs(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Query(query): Query<ListRunsQuery>,
) -> Response {
    json_result(automation::list_runs(&state.store, &project, query.limit).await)
}

pub(crate) async fn get_run_log(
    Extension(state): Extension<AppState>,
    Path((project, run_id)): Path<(String, i64)>,
) -> Response {
    json_result(automation::read_run_log(&state.store, &project, run_id).await)
}

async fn update_item_inner(
    state: &AppState,
    project: &str,
    item_id: i64,
    request: UpdateWorkItemRequest,
) -> Result<patchbay_types::WorkItemView> {
    let has_item_update = request.title.is_some()
        || request.description.is_some()
        || request.automation_claimable.is_some()
        || request.agent_model_override.is_some()
        || request.agent_reasoning_effort_override.is_some();

    if !has_item_update && request.state.is_none() {
        bail!("item update requires at least one field");
    }

    let mut updated = None;
    if has_item_update {
        updated = Some(
            items::update_item(
                &state.store,
                project,
                item_id,
                UpdateWorkItem {
                    title: request.title,
                    description: request.description,
                    automation_claimable: request.automation_claimable,
                    agent_model_override: request.agent_model_override,
                    agent_reasoning_effort_override: request.agent_reasoning_effort_override,
                    expect_version: request.expect_version,
                },
            )
            .await?,
        );
    }

    if let Some(state_filter) = request.state {
        updated = Some(
            items::move_item(
                &state.store,
                project,
                item_id,
                state_filter,
                (!has_item_update)
                    .then_some(request.expect_version)
                    .flatten(),
            )
            .await?,
        );
    }

    updated.ok_or_else(|| anyhow::anyhow!("item update requires at least one field"))
}

fn json_result<T>(result: Result<T>) -> Response
where
    T: Serialize,
{
    match result {
        Ok(value) => Json(value).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: err.to_string(),
            }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{Body, to_bytes};
    use patchbay_types::{
        ClaimWorkItemResponse, CommentView, ProjectMemoryCompactionView, ProjectMemoryEventView,
        ProjectMemoryUpdateView, ProjectMemoryView, WorkItemView,
    };
    use serde::de::DeserializeOwned;
    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::backend::{
        automation_controller::AutomationController,
        process_sessions::ProcessSessionRegistry,
        projects::{CreateProject, create_project},
        storage::{Store, utc_now},
    };

    async fn test_state() -> (TempDir, AppState, i64) {
        let temp = tempdir().unwrap();
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
                system_prompt: None,
                memory: None,
            },
        )
        .await
        .unwrap();
        let item = items::create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Endpoint work".to_owned(),
                description: "Exercise workflow API endpoints".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        let state = AppState {
            store,
            sessions: ProcessSessionRegistry::new(),
            automation_controller: AutomationController::new(),
            codex_status: Arc::new(tokio::sync::RwLock::new(
                patchbay_types::CodexAppServerStatusView {
                    available: true,
                    usable: true,
                    message: String::new(),
                    install_prompt: String::new(),
                    checked_at: utc_now(),
                    ..Default::default()
                },
            )),
        };
        (temp, state, item.id)
    }

    async fn decode<T: DeserializeOwned>(response: Response<Body>) -> T {
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn workflow_endpoints_claim_progress_release_and_finish() {
        let (_temp, state, item_id) = test_state().await;
        let agent_id = "patchbay-run-1".to_owned();

        let claimed: ClaimWorkItemResponse = decode(
            claim_item(
                Extension(state.clone()),
                Path("demo".to_owned()),
                Json(ClaimWorkItemRequest {
                    agent_id: agent_id.clone(),
                    state: WorkState::Open,
                }),
            )
            .await,
        )
        .await;
        let claimed_item = claimed.item.unwrap();
        assert_eq!(claimed_item.id, item_id);
        assert_eq!(claimed_item.claimed_by.as_deref(), Some(agent_id.as_str()));

        let progress: CommentView = decode(
            progress_item(
                Extension(state.clone()),
                Path(("demo".to_owned(), item_id)),
                Json(ProgressWorkItemRequest {
                    agent_id: agent_id.clone(),
                    body: "Working".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(progress.body, "Working");

        let released: WorkItemView = decode(
            release_item(
                Extension(state.clone()),
                Path(("demo".to_owned(), item_id)),
                Json(ReleaseWorkItemRequest {
                    agent_id: agent_id.clone(),
                    comment: Some("Paused".to_owned()),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(released.state, WorkState::Open);
        assert_eq!(released.claimed_by, None);

        let claimed: ClaimWorkItemResponse = decode(
            claim_item(
                Extension(state.clone()),
                Path("demo".to_owned()),
                Json(ClaimWorkItemRequest {
                    agent_id: agent_id.clone(),
                    state: WorkState::Open,
                }),
            )
            .await,
        )
        .await;
        assert!(claimed.claimed());

        let finished: WorkItemView = decode(
            finish_item(
                Extension(state),
                Path(("demo".to_owned(), item_id)),
                Json(FinishWorkItemRequest {
                    agent_id,
                    report: "Done".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(finished.state, WorkState::Done);
        assert_eq!(finished.claimed_by, None);
    }

    #[tokio::test]
    async fn memory_endpoints_snapshot_agent_changes_and_compact_history() {
        let (_temp, state, _item_id) = test_state().await;

        let set: ProjectMemoryUpdateView = decode(
            set_project_memory(
                Extension(state.clone()),
                Path("demo".to_owned()),
                Json(UpdateProjectMemoryRequest {
                    agent_id: "patchbay-run-7".to_owned(),
                    agent_run_id: None,
                    body: "Remember the relay CLI.".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(set.project.memory, "Remember the relay CLI.");
        assert_eq!(set.event.operation, "set");
        assert_eq!(set.event.memory, "Remember the relay CLI.");
        assert_eq!(set.event.actor_type.as_deref(), Some("agent"));
        assert_eq!(set.event.actor_id.as_deref(), Some("patchbay-run-7"));
        assert_eq!(set.event.agent_run_id, Some(7));

        let appended: ProjectMemoryUpdateView = decode(
            append_project_memory(
                Extension(state.clone()),
                Path("demo".to_owned()),
                Json(UpdateProjectMemoryRequest {
                    agent_id: "patchbay-run-7".to_owned(),
                    agent_run_id: None,
                    body: "Use Patchbay memory commands.".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(
            appended.project.memory,
            "Remember the relay CLI.\n\nUse Patchbay memory commands."
        );
        assert_eq!(appended.event.operation, "append");

        let current: ProjectMemoryView =
            decode(get_project_memory(Extension(state.clone()), Path("demo".to_owned())).await)
                .await;
        assert_eq!(current.last_event.unwrap().id, appended.event.id);

        let events: Vec<ProjectMemoryEventView> = decode(
            list_project_memory_events(Extension(state.clone()), Path("demo".to_owned())).await,
        )
        .await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, appended.event.id);

        let compacted: ProjectMemoryCompactionView = decode(
            compact_project_memory_events(Extension(state.clone()), Path("demo".to_owned())).await,
        )
        .await;
        assert_eq!(compacted.deleted_events, 2);

        let events: Vec<ProjectMemoryEventView> = decode(
            list_project_memory_events(Extension(state.clone()), Path("demo".to_owned())).await,
        )
        .await;
        assert!(events.is_empty());

        let current: ProjectMemoryView =
            decode(get_project_memory(Extension(state), Path("demo".to_owned())).await).await;
        assert_eq!(
            current.memory,
            "Remember the relay CLI.\n\nUse Patchbay memory commands."
        );
        assert!(current.last_event.is_none());
    }
}
