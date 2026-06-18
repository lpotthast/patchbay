use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use patchbay_types::{
    AddCommentRequest, ApiError, ClaimWorkItemRequest, ClaimWorkItemResponse,
    CreateWorkItemLabelRequest, CreateWorkItemRelationshipRequest, CreateWorkItemRequest,
    DEFAULT_STATE_LABEL, FinishWorkItemRequest, ProgressWorkItemRequest, ReleaseWorkItemRequest,
    RequestFeedbackWorkItemRequest, UpdateProjectMemoryRequest, UpdateWorkItemLabelRequest,
    UpdateWorkItemRelationshipRequest, UpdateWorkItemRequest,
};
use rootcause::Result;
use serde::{Deserialize, Serialize};

use crate::backend::{
    app_state::AppState,
    automation, comments,
    comments::AddComment,
    item_label_service, items,
    items::{CreateWorkItem, UpdateWorkItem},
    projects, relationships,
};

#[derive(Debug, Deserialize)]
pub(crate) struct ListItemsQuery {
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LabelMutationQuery {
    expect_version: Option<i64>,
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
            projects::ProjectChangeSource::Agent {
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
            projects::ProjectChangeSource::Agent {
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

pub(crate) async fn list_project_labels(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
) -> Response {
    json_result(item_label_service::list_project_labels(&state.store, &project).await)
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
                state: request
                    .state
                    .unwrap_or_else(|| DEFAULT_STATE_LABEL.to_owned()),
                agent_model_override: request.agent_model_override,
                agent_reasoning_effort_override: request.agent_reasoning_effort_override,
                initial_labels: request.initial_labels,
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
    json_result(
        items::update_item(
            &state.store,
            &project,
            item_id,
            UpdateWorkItem {
                title: request.title,
                description: request.description,
                state: request.state,
                agent_model_override: request.agent_model_override,
                agent_reasoning_effort_override: request.agent_reasoning_effort_override,
                expect_version: request.expect_version,
            },
        )
        .await,
    )
}

pub(crate) async fn claim_item(
    Extension(state): Extension<AppState>,
    Path(project): Path<String>,
    Json(request): Json<ClaimWorkItemRequest>,
) -> Response {
    json_result(
        items::claim_item(&state.store, &project, &request.agent_id, &request.state)
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
            items::ReleaseAutomationDisposition::Blocked,
        )
        .await,
    )
}

pub(crate) async fn request_item_feedback(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Json(request): Json<RequestFeedbackWorkItemRequest>,
) -> Response {
    json_result(
        items::request_feedback(
            &state.store,
            &project,
            item_id,
            &request.agent_id,
            &request.body,
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
    json_result(
        automation::read_run_log_with_active_session(
            &state.store,
            &state.sessions,
            &project,
            run_id,
        )
        .await,
    )
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

pub(crate) async fn list_item_labels(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
) -> Response {
    json_result(item_label_service::list_item_labels(&state.store, &project, item_id).await)
}

pub(crate) async fn add_item_label(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Query(query): Query<LabelMutationQuery>,
    Json(request): Json<CreateWorkItemLabelRequest>,
) -> Response {
    json_result(
        item_label_service::add_label(
            &state.store,
            &project,
            item_id,
            request.key,
            request.value,
            query.expect_version,
        )
        .await,
    )
}

pub(crate) async fn update_item_label(
    Extension(state): Extension<AppState>,
    Path((project, item_id, label_id)): Path<(String, i64, i64)>,
    Json(request): Json<UpdateWorkItemLabelRequest>,
) -> Response {
    json_result(
        item_label_service::update_label(
            &state.store,
            &project,
            item_id,
            label_id,
            request.key,
            request.value,
            request.expect_version,
        )
        .await,
    )
}

pub(crate) async fn delete_item_label(
    Extension(state): Extension<AppState>,
    Path((project, item_id, label_id)): Path<(String, i64, i64)>,
    Query(query): Query<LabelMutationQuery>,
) -> Response {
    json_result(
        item_label_service::delete_label(
            &state.store,
            &project,
            item_id,
            label_id,
            query.expect_version,
        )
        .await,
    )
}

pub(crate) async fn list_item_relationships(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
) -> Response {
    json_result(relationships::list_item_relationships(&state.store, &project, item_id).await)
}

pub(crate) async fn create_item_relationship(
    Extension(state): Extension<AppState>,
    Path((project, item_id)): Path<(String, i64)>,
    Json(request): Json<CreateWorkItemRelationshipRequest>,
) -> Response {
    json_result(
        relationships::create_relationship(
            &state.store,
            &project,
            item_id,
            request.target_work_item_id,
            request.kind,
        )
        .await,
    )
}

pub(crate) async fn update_relationship(
    Extension(state): Extension<AppState>,
    Path((project, relationship_id)): Path<(String, i64)>,
    Json(request): Json<UpdateWorkItemRelationshipRequest>,
) -> Response {
    json_result(
        relationships::update_relationship(&state.store, &project, relationship_id, request.kind)
            .await,
    )
}

pub(crate) async fn delete_relationship(
    Extension(state): Extension<AppState>,
    Path((project, relationship_id)): Path<(String, i64)>,
) -> Response {
    json_result(relationships::delete_relationship(&state.store, &project, relationship_id).await)
}

pub(crate) async fn update_item_relationship(
    Extension(state): Extension<AppState>,
    Path((project, item_id, relationship_id)): Path<(String, i64, i64)>,
    Json(request): Json<UpdateWorkItemRelationshipRequest>,
) -> Response {
    json_result(
        relationships::update_relationship_for_item(
            &state.store,
            &project,
            item_id,
            relationship_id,
            request.kind,
        )
        .await,
    )
}

pub(crate) async fn delete_item_relationship(
    Extension(state): Extension<AppState>,
    Path((project, item_id, relationship_id)): Path<(String, i64, i64)>,
) -> Response {
    json_result(
        relationships::delete_relationship_for_item(
            &state.store,
            &project,
            item_id,
            relationship_id,
        )
        .await,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{Body, to_bytes};
    use patchbay_types::{
        AUTOMATION_BLOCKED_LABEL_KEY, ClaimWorkItemResponse, CommentView,
        CreateWorkItemRelationshipRequest, DeleteWorkItemRelationshipResponse,
        FEEDBACK_REQUESTED_LABEL_KEY, ProjectLabelView, ProjectMemoryCompactionView,
        ProjectMemoryEventView, ProjectMemoryUpdateView, ProjectMemoryView,
        UpdateWorkItemRelationshipRequest, WorkItemLabelView, WorkItemRelationshipDirection,
        WorkItemRelationshipListEntry, WorkItemRelationshipView, WorkItemView,
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
                default_agent_reasoning_effort: None,
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
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
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

    async fn decode_error(response: Response<Body>) -> String {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice::<ApiError>(&body).unwrap().error
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
                    state: "open".to_owned(),
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
        assert_eq!(released.state.as_deref(), Some("open"));
        assert_eq!(released.claimed_by, None);
        assert!(
            released
                .labels
                .iter()
                .any(|label| label.key == AUTOMATION_BLOCKED_LABEL_KEY)
        );

        let claimed: ClaimWorkItemResponse = decode(
            claim_item(
                Extension(state.clone()),
                Path("demo".to_owned()),
                Json(ClaimWorkItemRequest {
                    agent_id: agent_id.clone(),
                    state: "open".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert!(!claimed.claimed());

        let feedback_item_id = items::create_item(
            &state.store,
            "demo",
            CreateWorkItem {
                title: "Endpoint feedback".to_owned(),
                description: "Exercise feedback request endpoint".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap()
        .id;

        let claimed: ClaimWorkItemResponse = decode(
            claim_item(
                Extension(state.clone()),
                Path("demo".to_owned()),
                Json(ClaimWorkItemRequest {
                    agent_id: agent_id.clone(),
                    state: "open".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(claimed.item.unwrap().id, feedback_item_id);

        let feedback_requested: WorkItemView = decode(
            request_item_feedback(
                Extension(state.clone()),
                Path(("demo".to_owned(), feedback_item_id)),
                Json(RequestFeedbackWorkItemRequest {
                    agent_id: agent_id.clone(),
                    body: "Need a user decision".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(feedback_requested.state.as_deref(), Some("open"));
        assert_eq!(feedback_requested.claimed_by, None);
        assert!(
            feedback_requested
                .labels
                .iter()
                .any(|label| label.key == FEEDBACK_REQUESTED_LABEL_KEY)
        );

        let finish_item_id = items::create_item(
            &state.store,
            "demo",
            CreateWorkItem {
                title: "Endpoint finish".to_owned(),
                description: "Exercise finish endpoint".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap()
        .id;

        let claimed: ClaimWorkItemResponse = decode(
            claim_item(
                Extension(state.clone()),
                Path("demo".to_owned()),
                Json(ClaimWorkItemRequest {
                    agent_id: agent_id.clone(),
                    state: "open".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(claimed.item.unwrap().id, finish_item_id);

        let finished: WorkItemView = decode(
            finish_item(
                Extension(state),
                Path(("demo".to_owned(), finish_item_id)),
                Json(FinishWorkItemRequest {
                    agent_id,
                    report: "Done".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(finished.state.as_deref(), Some("done"));
        assert_eq!(finished.claimed_by, None);
    }

    #[tokio::test]
    async fn update_endpoint_applies_fields_and_state_as_one_patch() {
        let (_temp, state, item_id) = test_state().await;
        let original = items::get_item(&state.store, "demo", item_id)
            .await
            .unwrap();

        let updated: WorkItemView = decode(
            update_item(
                Extension(state),
                Path(("demo".to_owned(), item_id)),
                Json(UpdateWorkItemRequest {
                    title: Some("Endpoint update".to_owned()),
                    description: None,
                    state: Some("review".to_owned()),
                    agent_model_override: None,
                    agent_reasoning_effort_override: None,
                    expect_version: Some(original.version),
                }),
            )
            .await,
        )
        .await;

        assert_eq!(updated.title, "Endpoint update");
        assert_eq!(updated.state.as_deref(), Some("review"));
        assert_eq!(updated.version, original.version + 1);
    }

    #[tokio::test]
    async fn create_endpoint_defaults_missing_labels_and_accepts_initial_labels() {
        let (_temp, state, _item_id) = test_state().await;
        let backwards_compatible: CreateWorkItemRequest =
            serde_json::from_value(serde_json::json!({
                "title": "No labels request",
                "description": "Older client payload",
                "state": "open",
                "agent_model_override": null,
                "agent_reasoning_effort_override": null
            }))
            .unwrap();

        let created_without_labels: WorkItemView = decode(
            create_item(
                Extension(state.clone()),
                Path("demo".to_owned()),
                Json(backwards_compatible),
            )
            .await,
        )
        .await;
        assert_eq!(created_without_labels.state.as_deref(), Some("open"));
        assert_eq!(
            created_without_labels
                .labels
                .iter()
                .filter(|label| label.key != patchbay_types::STATE_LABEL_KEY)
                .count(),
            0
        );

        let created_with_labels: WorkItemView = decode(
            create_item(
                Extension(state),
                Path("demo".to_owned()),
                Json(CreateWorkItemRequest {
                    title: "Initial labels request".to_owned(),
                    description: "New client payload".to_owned(),
                    state: Some("review".to_owned()),
                    agent_model_override: None,
                    agent_reasoning_effort_override: None,
                    initial_labels: vec![
                        CreateWorkItemLabelRequest {
                            key: "type".to_owned(),
                            value: Some("feature".to_owned()),
                        },
                        CreateWorkItemLabelRequest {
                            key: "needs-verification".to_owned(),
                            value: None,
                        },
                    ],
                }),
            )
            .await,
        )
        .await;

        assert_eq!(created_with_labels.state.as_deref(), Some("review"));
        assert!(
            created_with_labels
                .labels
                .iter()
                .any(|label| { label.key == "type" && label.value.as_deref() == Some("feature") })
        );
        assert!(
            created_with_labels
                .labels
                .iter()
                .any(|label| label.key == "needs-verification" && label.value.is_none())
        );
    }

    #[tokio::test]
    async fn label_endpoints_add_update_delete_and_suggest() {
        let (_temp, state, item_id) = test_state().await;

        let labeled: WorkItemView = decode(
            add_item_label(
                Extension(state.clone()),
                Path(("demo".to_owned(), item_id)),
                Query(LabelMutationQuery {
                    expect_version: None,
                }),
                Json(CreateWorkItemLabelRequest {
                    key: "severity".to_owned(),
                    value: Some("high".to_owned()),
                }),
            )
            .await,
        )
        .await;
        let label = labeled
            .labels
            .iter()
            .find(|label| label.key == "severity")
            .cloned()
            .unwrap();

        let labels: Vec<WorkItemLabelView> = decode(
            list_item_labels(Extension(state.clone()), Path(("demo".to_owned(), item_id))).await,
        )
        .await;
        assert!(labels.iter().any(|label| label.key == "severity"));

        let updated: WorkItemView = decode(
            update_item_label(
                Extension(state.clone()),
                Path(("demo".to_owned(), item_id, label.id)),
                Json(UpdateWorkItemLabelRequest {
                    key: Some("priority".to_owned()),
                    value: Some(Some("p1".to_owned())),
                    expect_version: None,
                }),
            )
            .await,
        )
        .await;
        assert!(
            updated
                .labels
                .iter()
                .any(|label| { label.key == "priority" && label.value.as_deref() == Some("p1") })
        );

        let suggestions: Vec<ProjectLabelView> =
            decode(list_project_labels(Extension(state.clone()), Path("demo".to_owned())).await)
                .await;
        assert!(
            suggestions
                .iter()
                .any(|label| { label.key == "priority" && label.value.as_deref() == Some("p1") })
        );

        let deleted = delete_item_label(
            Extension(state),
            Path(("demo".to_owned(), item_id, label.id)),
            Query(LabelMutationQuery {
                expect_version: None,
            }),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn relationship_endpoints_create_list_update_delete_and_validate() {
        let (_temp, state, source_id) = test_state().await;
        let target_id = items::create_item(
            &state.store,
            "demo",
            CreateWorkItem {
                title: "Relationship target".to_owned(),
                description: "Receives a relationship".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap()
        .id;

        let created: WorkItemRelationshipListEntry = decode(
            create_item_relationship(
                Extension(state.clone()),
                Path(("demo".to_owned(), source_id)),
                Json(CreateWorkItemRelationshipRequest {
                    target_work_item_id: target_id,
                    kind: " is follow-up of ".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(created.direction, WorkItemRelationshipDirection::Outgoing);
        assert_eq!(created.relationship.kind, "is follow-up of");

        let outgoing: Vec<WorkItemRelationshipListEntry> = decode(
            list_item_relationships(
                Extension(state.clone()),
                Path(("demo".to_owned(), source_id)),
            )
            .await,
        )
        .await;
        assert_eq!(outgoing.len(), 1);
        assert_eq!(
            outgoing[0].direction,
            WorkItemRelationshipDirection::Outgoing
        );

        let incoming: Vec<WorkItemRelationshipListEntry> = decode(
            list_item_relationships(
                Extension(state.clone()),
                Path(("demo".to_owned(), target_id)),
            )
            .await,
        )
        .await;
        assert_eq!(incoming.len(), 1);
        assert_eq!(
            incoming[0].direction,
            WorkItemRelationshipDirection::Incoming
        );

        let duplicate = decode_error(
            create_item_relationship(
                Extension(state.clone()),
                Path(("demo".to_owned(), source_id)),
                Json(CreateWorkItemRelationshipRequest {
                    target_work_item_id: target_id,
                    kind: "is follow-up of".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert!(duplicate.contains("duplicate relationship"));

        let self_link = decode_error(
            create_item_relationship(
                Extension(state.clone()),
                Path(("demo".to_owned(), source_id)),
                Json(CreateWorkItemRelationshipRequest {
                    target_work_item_id: source_id,
                    kind: "relates".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert!(self_link.contains("must differ"));

        let empty_kind = decode_error(
            create_item_relationship(
                Extension(state.clone()),
                Path(("demo".to_owned(), source_id)),
                Json(CreateWorkItemRelationshipRequest {
                    target_work_item_id: target_id,
                    kind: " ".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert!(empty_kind.contains("kind cannot be empty"));

        let updated: WorkItemRelationshipView = decode(
            update_relationship(
                Extension(state.clone()),
                Path(("demo".to_owned(), created.relationship.id)),
                Json(UpdateWorkItemRelationshipRequest {
                    kind: "unblocks".to_owned(),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(updated.kind, "unblocks");

        let deleted: DeleteWorkItemRelationshipResponse = decode(
            delete_relationship(
                Extension(state.clone()),
                Path(("demo".to_owned(), created.relationship.id)),
            )
            .await,
        )
        .await;
        assert!(deleted.deleted);

        let outgoing: Vec<WorkItemRelationshipListEntry> = decode(
            list_item_relationships(Extension(state), Path(("demo".to_owned(), source_id))).await,
        )
        .await;
        assert!(outgoing.is_empty());
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
