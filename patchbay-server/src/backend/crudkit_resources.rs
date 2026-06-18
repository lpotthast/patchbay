use std::{convert::Infallible, fmt, path::PathBuf, sync::Arc};

use crudkit_core::collaboration::CollabMessage;
use crudkit_rs::{
    collaboration::CollaborationService, lifetime::NoopLifetimeHooks, prelude::*,
    resource::ResourceType, validate::GlobalValidationState,
};
use crudkit_sea_orm::{CrudColumns, SeaOrmResource, repo::SeaOrmRepo};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
use utoipa::ToSchema;

use crate::{
    backend::{
        automation_triggers,
        entities::{
            agent_run, agent_tool, automation_trigger, comment, project, swim_lane, work_item,
            work_item_label, work_item_state,
        },
        events, projects,
        storage::{Store, utc_now},
        swim_lanes, work_item_events, work_item_states,
    },
    shared::view_models::{
        AgentReasoningEffort, AgentSandboxMode, AgentToolName, AutomationActivation,
        AutomationEffect, AutomationRunMutability, CodexAgentModel, DEFAULT_STATE_LABEL,
        RevertStrategy, STATE_LABEL_KEY, WorkspaceMode, WorktreeCleanupPolicy,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrudResources {
    Project,
    WorkItem,
    Comment,
    AgentTool,
    AgentRun,
    AutomationTrigger,
    SwimLane,
    WorkItemState,
}

impl ResourceType for CrudResources {
    fn name(&self) -> &'static str {
        match self {
            Self::Project => "projects",
            Self::WorkItem => "work_items",
            Self::Comment => "comments",
            Self::AgentTool => "agent_tools",
            Self::AgentRun => "agent_runs",
            Self::AutomationTrigger => "automation_triggers",
            Self::SwimLane => "swim_lanes",
            Self::WorkItemState => "work_item_states",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NoopCollaborationService;

impl CollaborationService for NoopCollaborationService {
    type Error = Infallible;

    async fn broadcast_json(&self, _json: CollabMessage) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct ProjectResourceContext {
    store: Store,
}

impl fmt::Debug for ProjectResourceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProjectResourceContext")
    }
}

impl CrudResourceContext for ProjectResourceContext {}

#[derive(Debug, Clone)]
pub struct ProjectHookError(String);

impl fmt::Display for ProjectHookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProjectHookError {}

#[derive(Debug, Default)]
pub struct ProjectHookData {
    previous_memory: Option<String>,
}

#[derive(Debug)]
pub struct ProjectLifetime;

impl CrudLifetime<CrudProjectResource> for ProjectLifetime {
    type Error = ProjectHookError;

    async fn before_read(
        _read_request: &mut ReadRequest<CrudProjectResource>,
        _context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_read(
        _read_request: &ReadRequest<CrudProjectResource>,
        _read_result: &mut ReadResult<CrudProjectResource>,
        _context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
        Ok(data)
    }

    async fn before_create(
        create_model: &mut project::CreateModel,
        _context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
        create_model.path = Some(normalize_crud_project_path(create_model.path.take())?);
        create_model.default_agent_model =
            projects::normalize_optional(create_model.default_agent_model.take())
                .or_else(|| Some(CodexAgentModel::newest().as_storage().to_owned()));
        projects::validate_agent_model(create_model.default_agent_model.as_deref())
            .map_err(|err| project_unprocessable_error(err.to_string()))?;
        create_model.default_agent_reasoning_effort = create_model
            .default_agent_reasoning_effort
            .take()
            .and_then(|effort| projects::normalize_optional(Some(effort)))
            .map(|effort| {
                effort
                    .parse::<AgentReasoningEffort>()
                    .map(|effort| effort.as_storage().to_owned())
                    .map_err(|err| project_unprocessable_error(err.to_string()))
            })
            .transpose()?
            .or_else(|| Some(AgentReasoningEffort::highest().as_storage().to_owned()));
        Ok(data)
    }

    async fn after_create(
        _create_model: &project::CreateModel,
        model: &project::Model,
        context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
        refresh_crud_project_path_status(context, model).await?;
        work_item_states::ensure_default_work_item_states_for_project_id(&context.store, model.id)
            .await
            .map_err(|err| ProjectHookError(err.to_string()))
            .map_err(HookError::Internal)?;
        swim_lanes::ensure_default_swim_lanes_for_project_id(&context.store, model.id)
            .await
            .map_err(|err| ProjectHookError(err.to_string()))
            .map_err(HookError::Internal)?;
        automation_triggers::ensure_default_project_automations(
            &context.store,
            model.id,
            &model.default_agent_tool,
        )
        .await
        .map_err(|err| ProjectHookError(err.to_string()))
        .map_err(HookError::Internal)?;
        if !model.memory.trim().is_empty() {
            projects::snapshot_current_memory_event(
                &context.store,
                &model.name,
                "initial",
                projects::ProjectChangeSource::User,
            )
            .await
            .map_err(|err| ProjectHookError(err.to_string()))
            .map_err(HookError::Internal)?;
        }
        events::publish_project_list_changed();
        events::publish_project_changed(&model.name);
        Ok(data)
    }

    async fn before_update(
        existing: &project::Model,
        update_model: &mut project::UpdateModel,
        _update_request: &UpdateRequest,
        _context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        mut data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
        data.previous_memory = Some(existing.memory.clone());
        if update_model.path.is_some() {
            update_model.path = Some(normalize_crud_project_path(update_model.path.take())?);
        }
        update_model.default_agent_model =
            projects::normalize_optional(update_model.default_agent_model.take());
        projects::validate_agent_model(update_model.default_agent_model.as_deref())
            .map_err(|err| project_unprocessable_error(err.to_string()))?;
        let workspace_mode = update_model
            .workspace_mode
            .parse::<WorkspaceMode>()
            .map_err(|err| project_unprocessable_error(err.to_string()))?;
        let worktree_cleanup_policy = update_model
            .worktree_cleanup_policy
            .parse::<WorktreeCleanupPolicy>()
            .map_err(|err| project_unprocessable_error(err.to_string()))?;
        update_model.worktree_cleanup_policy = worktree_cleanup_policy.as_storage().to_owned();
        update_model.commit_standard = update_model.commit_standard.trim().to_owned();
        update_model.revert_strategy = update_model
            .revert_strategy
            .parse::<RevertStrategy>()
            .map_err(|err| project_unprocessable_error(err.to_string()))?
            .as_storage()
            .to_owned();
        update_model.default_agent_tool = update_model
            .default_agent_tool
            .parse::<AgentToolName>()
            .map_err(|err| project_unprocessable_error(err.to_string()))?
            .as_storage()
            .to_owned();
        update_model.default_agent_reasoning_effort = update_model
            .default_agent_reasoning_effort
            .take()
            .and_then(|effort| projects::normalize_optional(Some(effort)))
            .map(|effort| {
                effort
                    .parse::<AgentReasoningEffort>()
                    .map(|effort| effort.as_storage().to_owned())
                    .map_err(|err| project_unprocessable_error(err.to_string()))
            })
            .transpose()?;
        update_model.agent_sandbox_mode = update_model
            .agent_sandbox_mode
            .parse::<AgentSandboxMode>()
            .map_err(|err| project_unprocessable_error(err.to_string()))?
            .as_storage()
            .to_owned();
        let agent_extra_writable_roots = projects::parse_agent_extra_writable_roots_text(
            &update_model.agent_extra_writable_roots,
        )
        .map_err(|err| project_unprocessable_error(err.to_string()))?;
        projects::validate_agent_extra_writable_roots_do_not_include_database(
            &agent_extra_writable_roots,
            _context.store.path(),
        )
        .map_err(|err| project_unprocessable_error(err.to_string()))?;
        update_model.agent_extra_writable_roots =
            projects::serialize_agent_extra_writable_roots(&agent_extra_writable_roots);
        projects::validate_settings(
            workspace_mode,
            update_model.max_code_edit_agents,
            update_model.max_read_only_agents,
            update_model.create_pr,
            update_model.stale_claim_minutes,
            update_model.default_agent_model.as_deref(),
        )
        .map_err(|err| project_unprocessable_error(err.to_string()))?;
        Ok(data)
    }

    async fn after_update(
        _update_model: &project::UpdateModel,
        model: &project::Model,
        _update_request: &UpdateRequest,
        context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
        refresh_crud_project_path_status(context, model).await?;
        if data
            .previous_memory
            .as_deref()
            .is_some_and(|previous| previous != model.memory)
        {
            projects::snapshot_current_memory_event(
                &context.store,
                &model.name,
                "set",
                projects::ProjectChangeSource::User,
            )
            .await
            .map_err(|err| ProjectHookError(err.to_string()))
            .map_err(HookError::Internal)?;
        }
        events::publish_project_list_changed();
        events::publish_project_changed(&model.name);
        Ok(data)
    }

    async fn before_delete(
        _model: &project::Model,
        _delete_request: &DeleteRequest<CrudProjectResource>,
        _context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_delete(
        model: &project::Model,
        _delete_request: &DeleteRequest<CrudProjectResource>,
        _context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
        events::publish_project_list_changed();
        events::publish_project_changed(&model.name);
        Ok(data)
    }
}

#[derive(Debug, ToSchema)]
pub struct CrudProjectResource;

impl CrudResource for CrudProjectResource {
    type ReadModel = project::read_view::Model;
    type ReadModelId = project::read_view::ModelId;
    type ReadModelField = project::read_view::ModelField;

    type CreateModel = project::CreateModel;
    type CreateModelField = project::ModelField;

    type UpdateModel = project::UpdateModel;
    type UpdateModelField = project::ModelField;

    type Model = project::Model;
    type Id = project::ProjectId;
    type ModelField = project::ModelField;

    type Repository = SeaOrmRepo;
    type ValidationResultRepository =
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository;
    type CollaborationService = NoopCollaborationService;
    type Context = ProjectResourceContext;
    type HookData = ProjectHookData;
    type Lifetime = ProjectLifetime;
    type Auth = NoAuth;
    type AuthPolicy = OpenAuthPolicy;
    type ResourceType = CrudResources;
    const TYPE: CrudResources = CrudResources::Project;
}

impl SeaOrmResource for CrudProjectResource {
    type Entity = project::Entity;
    type SeaOrmModel = project::Model;
    type ActiveModel = project::ActiveModel;
    type Column = project::Column;
    type PrimaryKey = <project::Entity as EntityTrait>::PrimaryKey;

    type ReadViewEntity = project::read_view::Entity;
    type ReadViewSeaOrmModel = project::read_view::Model;
    type ReadViewActiveModel = project::read_view::ActiveModel;
    type ReadViewColumn = project::read_view::Column;
    type ReadViewPrimaryKey = <project::read_view::Entity as EntityTrait>::PrimaryKey;

    fn model_field_to_column(field: &Self::ModelField) -> Self::Column {
        <project::ModelField as CrudColumns<project::Column>>::to_sea_orm_column(field)
    }

    fn read_model_field_to_column(field: &Self::ReadModelField) -> Self::ReadViewColumn {
        <project::read_view::ModelField as CrudColumns<project::read_view::Column>>::to_sea_orm_column(field)
    }
}

fn normalize_crud_project_path(
    path: Option<String>,
) -> Result<String, HookError<ProjectHookError>> {
    projects::normalize_project_path(PathBuf::from(path.unwrap_or_default())).map_err(|err| {
        HookError::UnprocessableEntity {
            reason: err.to_string(),
        }
    })
}

fn project_unprocessable_error(reason: String) -> HookError<ProjectHookError> {
    HookError::UnprocessableEntity { reason }
}

async fn refresh_crud_project_path_status(
    context: &ProjectResourceContext,
    model: &project::Model,
) -> Result<(), HookError<ProjectHookError>> {
    projects::refresh_project_path_status(&context.store, model.id)
        .await
        .map(|_| ())
        .map_err(|err| HookError::Internal(ProjectHookError(err.to_string())))
}

#[derive(Clone)]
pub struct WorkItemResourceContext {
    store: Store,
}

impl fmt::Debug for WorkItemResourceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkItemResourceContext")
    }
}

impl CrudResourceContext for WorkItemResourceContext {}

#[derive(Debug, Clone)]
pub struct WorkItemHookError(String);

impl fmt::Display for WorkItemHookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WorkItemHookError {}

#[derive(Debug)]
pub struct WorkItemLifetime;

impl CrudLifetime<CrudWorkItemResource> for WorkItemLifetime {
    type Error = WorkItemHookError;

    async fn before_read(
        _read_request: &mut ReadRequest<CrudWorkItemResource>,
        _context: &WorkItemResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_read(
        _read_request: &ReadRequest<CrudWorkItemResource>,
        _read_result: &mut ReadResult<CrudWorkItemResource>,
        _context: &WorkItemResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn before_create(
        create_model: &mut work_item::CreateModel,
        _context: &WorkItemResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        validate_work_item_text(&create_model.title, &create_model.description)?;
        create_model.agent_model_override =
            projects::normalize_optional(create_model.agent_model_override.take());
        create_model.agent_reasoning_effort_override = create_model
            .agent_reasoning_effort_override
            .take()
            .and_then(|effort| projects::normalize_optional(Some(effort)))
            .map(|effort| {
                effort
                    .parse::<AgentReasoningEffort>()
                    .map(|effort| effort.as_storage().to_owned())
                    .map_err(|err| work_item_unprocessable_error(err.to_string()))
            })
            .transpose()?;
        Ok(data)
    }

    async fn after_create(
        _create_model: &work_item::CreateModel,
        model: &work_item::Model,
        context: &WorkItemResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        let now = utc_now();
        let active = work_item_label::ActiveModel {
            project_id: Set(model.project_id),
            work_item_id: Set(model.id),
            key: Set(STATE_LABEL_KEY.to_owned()),
            value: Set(Some(DEFAULT_STATE_LABEL.to_owned())),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        };
        active
            .insert(context.store.db().as_ref())
            .await
            .map_err(work_item_internal_error)?;
        work_item_events::record_event_in_tx(
            context.store.db().as_ref(),
            model.project_id,
            Some(model.id),
            "item_created",
            "Created item",
        )
        .await
        .map_err(work_item_internal_error)?;
        publish_work_item_crud_event(&context.store, model.project_id, model.id).await;
        Ok(data)
    }

    async fn before_update(
        _existing: &work_item::Model,
        update_model: &mut work_item::UpdateModel,
        _update_request: &UpdateRequest,
        _context: &WorkItemResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        validate_work_item_text(&update_model.title, &update_model.description)?;
        update_model.agent_model_override =
            projects::normalize_optional(update_model.agent_model_override.take());
        update_model.agent_reasoning_effort_override = update_model
            .agent_reasoning_effort_override
            .take()
            .and_then(|effort| projects::normalize_optional(Some(effort)))
            .map(|effort| {
                effort
                    .parse::<AgentReasoningEffort>()
                    .map(|effort| effort.as_storage().to_owned())
                    .map_err(|err| work_item_unprocessable_error(err.to_string()))
            })
            .transpose()?;
        Ok(data)
    }

    async fn after_update(
        _update_model: &work_item::UpdateModel,
        model: &work_item::Model,
        _update_request: &UpdateRequest,
        context: &WorkItemResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        publish_work_item_crud_event(&context.store, model.project_id, model.id).await;
        Ok(data)
    }

    async fn before_delete(
        _model: &work_item::Model,
        _delete_request: &DeleteRequest<CrudWorkItemResource>,
        _context: &WorkItemResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_delete(
        model: &work_item::Model,
        _delete_request: &DeleteRequest<CrudWorkItemResource>,
        context: &WorkItemResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        publish_work_item_crud_event(&context.store, model.project_id, model.id).await;
        Ok(data)
    }
}

async fn publish_work_item_crud_event(store: &Store, project_id: i64, item_id: i64) {
    match projects::project_name_by_id(store, project_id).await {
        Ok(project_name) => events::publish_work_item_changed(&project_name, item_id),
        Err(err) => {
            tracing::warn!(
                project_id,
                item_id,
                error = %format_args!("{err:#}"),
                "failed to resolve project for work item UI event"
            );
        }
    }
}

fn validate_work_item_text(
    title: &str,
    description: &str,
) -> Result<(), HookError<WorkItemHookError>> {
    if title.trim().is_empty() {
        return Err(work_item_unprocessable_error(
            "item title cannot be empty".to_owned(),
        ));
    }
    if description.trim().is_empty() {
        return Err(work_item_unprocessable_error(
            "item description cannot be empty".to_owned(),
        ));
    }
    Ok(())
}

fn work_item_unprocessable_error(reason: String) -> HookError<WorkItemHookError> {
    HookError::UnprocessableEntity { reason }
}

fn work_item_internal_error(error: impl fmt::Display) -> HookError<WorkItemHookError> {
    HookError::Internal(WorkItemHookError(error.to_string()))
}

#[derive(Debug, ToSchema)]
pub struct CrudWorkItemResource;

impl CrudResource for CrudWorkItemResource {
    type ReadModel = work_item::read_view::Model;
    type ReadModelId = work_item::read_view::ModelId;
    type ReadModelField = work_item::read_view::ModelField;

    type CreateModel = work_item::CreateModel;
    type CreateModelField = work_item::ModelField;

    type UpdateModel = work_item::UpdateModel;
    type UpdateModelField = work_item::ModelField;

    type Model = work_item::Model;
    type Id = work_item::WorkItemId;
    type ModelField = work_item::ModelField;

    type Repository = SeaOrmRepo;
    type ValidationResultRepository =
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository;
    type CollaborationService = NoopCollaborationService;
    type Context = WorkItemResourceContext;
    type HookData = ();
    type Lifetime = WorkItemLifetime;
    type Auth = NoAuth;
    type AuthPolicy = OpenAuthPolicy;
    type ResourceType = CrudResources;
    const TYPE: CrudResources = CrudResources::WorkItem;
}

impl SeaOrmResource for CrudWorkItemResource {
    type Entity = work_item::Entity;
    type SeaOrmModel = work_item::Model;
    type ActiveModel = work_item::ActiveModel;
    type Column = work_item::Column;
    type PrimaryKey = <work_item::Entity as EntityTrait>::PrimaryKey;

    type ReadViewEntity = work_item::read_view::Entity;
    type ReadViewSeaOrmModel = work_item::read_view::Model;
    type ReadViewActiveModel = work_item::read_view::ActiveModel;
    type ReadViewColumn = work_item::read_view::Column;
    type ReadViewPrimaryKey = <work_item::read_view::Entity as EntityTrait>::PrimaryKey;

    fn model_field_to_column(field: &Self::ModelField) -> Self::Column {
        <work_item::ModelField as CrudColumns<work_item::Column>>::to_sea_orm_column(field)
    }

    fn read_model_field_to_column(field: &Self::ReadModelField) -> Self::ReadViewColumn {
        <work_item::read_view::ModelField as CrudColumns<work_item::read_view::Column>>::to_sea_orm_column(field)
    }
}

#[derive(Debug, CkResourceContext)]
pub struct CommentResourceContext;

#[derive(Debug, ToSchema)]
pub struct CrudCommentResource;

impl CrudResource for CrudCommentResource {
    type ReadModel = comment::read_view::Model;
    type ReadModelId = comment::read_view::ModelId;
    type ReadModelField = comment::read_view::ModelField;

    type CreateModel = comment::CreateModel;
    type CreateModelField = comment::ModelField;

    type UpdateModel = comment::UpdateModel;
    type UpdateModelField = comment::ModelField;

    type Model = comment::Model;
    type Id = comment::CommentId;
    type ModelField = comment::ModelField;

    type Repository = SeaOrmRepo;
    type ValidationResultRepository =
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository;
    type CollaborationService = NoopCollaborationService;
    type Context = CommentResourceContext;
    type HookData = ();
    type Lifetime = NoopLifetimeHooks;
    type Auth = NoAuth;
    type AuthPolicy = OpenAuthPolicy;
    type ResourceType = CrudResources;
    const TYPE: CrudResources = CrudResources::Comment;
}

impl SeaOrmResource for CrudCommentResource {
    type Entity = comment::Entity;
    type SeaOrmModel = comment::Model;
    type ActiveModel = comment::ActiveModel;
    type Column = comment::Column;
    type PrimaryKey = <comment::Entity as EntityTrait>::PrimaryKey;

    type ReadViewEntity = comment::read_view::Entity;
    type ReadViewSeaOrmModel = comment::read_view::Model;
    type ReadViewActiveModel = comment::read_view::ActiveModel;
    type ReadViewColumn = comment::read_view::Column;
    type ReadViewPrimaryKey = <comment::read_view::Entity as EntityTrait>::PrimaryKey;

    fn model_field_to_column(field: &Self::ModelField) -> Self::Column {
        <comment::ModelField as CrudColumns<comment::Column>>::to_sea_orm_column(field)
    }

    fn read_model_field_to_column(field: &Self::ReadModelField) -> Self::ReadViewColumn {
        <comment::read_view::ModelField as CrudColumns<comment::read_view::Column>>::to_sea_orm_column(field)
    }
}

#[derive(Debug, CkResourceContext)]
pub struct AgentToolResourceContext;

#[derive(Debug, ToSchema)]
pub struct CrudAgentToolResource;

impl CrudResource for CrudAgentToolResource {
    type ReadModel = agent_tool::read_view::Model;
    type ReadModelId = agent_tool::read_view::ModelId;
    type ReadModelField = agent_tool::read_view::ModelField;

    type CreateModel = agent_tool::CreateModel;
    type CreateModelField = agent_tool::ModelField;

    type UpdateModel = agent_tool::UpdateModel;
    type UpdateModelField = agent_tool::ModelField;

    type Model = agent_tool::Model;
    type Id = agent_tool::AgentToolId;
    type ModelField = agent_tool::ModelField;

    type Repository = SeaOrmRepo;
    type ValidationResultRepository =
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository;
    type CollaborationService = NoopCollaborationService;
    type Context = AgentToolResourceContext;
    type HookData = ();
    type Lifetime = NoopLifetimeHooks;
    type Auth = NoAuth;
    type AuthPolicy = OpenAuthPolicy;
    type ResourceType = CrudResources;
    const TYPE: CrudResources = CrudResources::AgentTool;
}

impl SeaOrmResource for CrudAgentToolResource {
    type Entity = agent_tool::Entity;
    type SeaOrmModel = agent_tool::Model;
    type ActiveModel = agent_tool::ActiveModel;
    type Column = agent_tool::Column;
    type PrimaryKey = <agent_tool::Entity as EntityTrait>::PrimaryKey;

    type ReadViewEntity = agent_tool::read_view::Entity;
    type ReadViewSeaOrmModel = agent_tool::read_view::Model;
    type ReadViewActiveModel = agent_tool::read_view::ActiveModel;
    type ReadViewColumn = agent_tool::read_view::Column;
    type ReadViewPrimaryKey = <agent_tool::read_view::Entity as EntityTrait>::PrimaryKey;

    fn model_field_to_column(field: &Self::ModelField) -> Self::Column {
        <agent_tool::ModelField as CrudColumns<agent_tool::Column>>::to_sea_orm_column(field)
    }

    fn read_model_field_to_column(field: &Self::ReadModelField) -> Self::ReadViewColumn {
        <agent_tool::read_view::ModelField as CrudColumns<agent_tool::read_view::Column>>::to_sea_orm_column(field)
    }
}

#[derive(Debug, CkResourceContext)]
pub struct AgentRunResourceContext;

#[derive(Debug, ToSchema)]
pub struct CrudAgentRunResource;

impl CrudResource for CrudAgentRunResource {
    type ReadModel = agent_run::read_view::Model;
    type ReadModelId = agent_run::read_view::ModelId;
    type ReadModelField = agent_run::read_view::ModelField;

    type CreateModel = agent_run::CreateModel;
    type CreateModelField = agent_run::ModelField;

    type UpdateModel = agent_run::UpdateModel;
    type UpdateModelField = agent_run::ModelField;

    type Model = agent_run::Model;
    type Id = agent_run::AgentRunId;
    type ModelField = agent_run::ModelField;

    type Repository = SeaOrmRepo;
    type ValidationResultRepository =
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository;
    type CollaborationService = NoopCollaborationService;
    type Context = AgentRunResourceContext;
    type HookData = ();
    type Lifetime = NoopLifetimeHooks;
    type Auth = NoAuth;
    type AuthPolicy = OpenAuthPolicy;
    type ResourceType = CrudResources;
    const TYPE: CrudResources = CrudResources::AgentRun;
}

impl SeaOrmResource for CrudAgentRunResource {
    type Entity = agent_run::Entity;
    type SeaOrmModel = agent_run::Model;
    type ActiveModel = agent_run::ActiveModel;
    type Column = agent_run::Column;
    type PrimaryKey = <agent_run::Entity as EntityTrait>::PrimaryKey;

    type ReadViewEntity = agent_run::read_view::Entity;
    type ReadViewSeaOrmModel = agent_run::read_view::Model;
    type ReadViewActiveModel = agent_run::read_view::ActiveModel;
    type ReadViewColumn = agent_run::read_view::Column;
    type ReadViewPrimaryKey = <agent_run::read_view::Entity as EntityTrait>::PrimaryKey;

    fn model_field_to_column(field: &Self::ModelField) -> Self::Column {
        <agent_run::ModelField as CrudColumns<agent_run::Column>>::to_sea_orm_column(field)
    }

    fn read_model_field_to_column(field: &Self::ReadModelField) -> Self::ReadViewColumn {
        <agent_run::read_view::ModelField as CrudColumns<agent_run::read_view::Column>>::to_sea_orm_column(field)
    }
}

#[derive(Clone)]
pub struct AutomationTriggerResourceContext {
    store: Store,
}

impl fmt::Debug for AutomationTriggerResourceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AutomationTriggerResourceContext")
    }
}

impl CrudResourceContext for AutomationTriggerResourceContext {}

#[derive(Debug, Clone, Default)]
pub struct AutomationTriggerHookData {
    next_evaluation_at: Option<String>,
    last_event_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AutomationTriggerHookError(String);

impl fmt::Display for AutomationTriggerHookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AutomationTriggerHookError {}

#[derive(Debug)]
pub struct AutomationTriggerLifetime;

impl CrudLifetime<CrudAutomationTriggerResource> for AutomationTriggerLifetime {
    type Error = AutomationTriggerHookError;

    async fn before_read(
        _read_request: &mut ReadRequest<CrudAutomationTriggerResource>,
        _context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_read(
        _read_request: &ReadRequest<CrudAutomationTriggerResource>,
        _read_result: &mut ReadResult<CrudAutomationTriggerResource>,
        _context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
        Ok(data)
    }

    async fn before_create(
        create_model: &mut automation_trigger::CreateModel,
        context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        _data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
        create_model.schedule =
            normalize_required_schedule(std::mem::take(&mut create_model.schedule))?;
        let activation = parse_activation(&create_model.activation)?;
        let effect = parse_effect(&create_model.effect)?;
        create_model.mutability = parse_mutability(&create_model.mutability)?
            .as_storage()
            .to_owned();
        create_model.work_item_selector =
            normalize_selector_storage(activation, create_model.work_item_selector.take())?;
        let selector = parse_work_item_selector(create_model.work_item_selector.as_deref())?;
        validate_trigger_configuration(
            &create_model.name,
            activation,
            effect,
            &create_model.schedule,
            selector.as_ref(),
            &create_model.prompt,
        )?;
        create_model.tool_name =
            default_tool_name_for_project(context, create_model.project_id).await?;
        trigger_hook_data(
            &context.store,
            create_model.project_id,
            activation,
            &create_model.schedule,
            None,
        )
        .await
    }

    async fn after_create(
        _create_model: &automation_trigger::CreateModel,
        model: &automation_trigger::Model,
        context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
        apply_trigger_hook_data(context, model, data.clone()).await?;
        publish_automation_project_event(&context.store, model.project_id).await;
        Ok(data)
    }

    async fn before_update(
        existing: &automation_trigger::Model,
        update_model: &mut automation_trigger::UpdateModel,
        _update_request: &UpdateRequest,
        context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        _data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
        update_model.schedule =
            normalize_required_schedule(std::mem::take(&mut update_model.schedule))?;
        let previous_activation = parse_activation(&existing.activation)?;
        let activation = parse_activation(&update_model.activation)?;
        let effect = parse_effect(&update_model.effect)?;
        update_model.mutability = parse_mutability(&update_model.mutability)?
            .as_storage()
            .to_owned();
        update_model.work_item_selector =
            normalize_selector_storage(activation, update_model.work_item_selector.take())?;
        let selector = parse_work_item_selector(update_model.work_item_selector.as_deref())?;
        validate_trigger_configuration(
            &update_model.name,
            activation,
            effect,
            &update_model.schedule,
            selector.as_ref(),
            &update_model.prompt,
        )?;
        update_model.tool_name =
            default_tool_name_for_project(context, existing.project_id).await?;
        trigger_hook_data(
            &context.store,
            existing.project_id,
            activation,
            &update_model.schedule,
            Some((previous_activation, existing.last_event_id)),
        )
        .await
    }

    async fn after_update(
        _update_model: &automation_trigger::UpdateModel,
        model: &automation_trigger::Model,
        _update_request: &UpdateRequest,
        context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
        apply_trigger_hook_data(context, model, data.clone()).await?;
        publish_automation_project_event(&context.store, model.project_id).await;
        Ok(data)
    }

    async fn before_delete(
        _model: &automation_trigger::Model,
        _delete_request: &DeleteRequest<CrudAutomationTriggerResource>,
        _context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_delete(
        model: &automation_trigger::Model,
        _delete_request: &DeleteRequest<CrudAutomationTriggerResource>,
        context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
        publish_automation_project_event(&context.store, model.project_id).await;
        Ok(data)
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn normalize_required_schedule(
    value: String,
) -> Result<String, HookError<AutomationTriggerHookError>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(trigger_unprocessable_error(
            "automation schedule is required".to_owned(),
        ));
    }
    automation_triggers::next_evaluation_at(trimmed)
        .map_err(|err| trigger_unprocessable_error(err.to_string()))?;
    Ok(trimmed.to_owned())
}

async fn publish_automation_project_event(store: &Store, project_id: i64) {
    match projects::project_name_by_id(store, project_id).await {
        Ok(project_name) => events::publish_automation_changed(&project_name),
        Err(err) => {
            tracing::warn!(
                project_id,
                error = %format_args!("{err:#}"),
                "failed to resolve project for automation UI event"
            );
        }
    }
}

async fn publish_swim_lane_project_event(store: &Store, project_id: i64) {
    match projects::project_name_by_id(store, project_id).await {
        Ok(project_name) => events::publish_swim_lane_changed(&project_name),
        Err(err) => {
            tracing::warn!(
                project_id,
                error = %format_args!("{err:#}"),
                "failed to resolve project for swim lane UI event"
            );
        }
    }
}

async fn publish_work_item_state_project_event(store: &Store, project_id: i64) {
    match projects::project_name_by_id(store, project_id).await {
        Ok(project_name) => events::publish_work_item_state_changed(&project_name),
        Err(err) => {
            tracing::warn!(
                project_id,
                error = %format_args!("{err:#}"),
                "failed to resolve project for work item state UI event"
            );
        }
    }
}

fn parse_activation(
    value: &str,
) -> Result<AutomationActivation, HookError<AutomationTriggerHookError>> {
    value
        .parse::<AutomationActivation>()
        .map_err(|err| trigger_unprocessable_error(err.to_string()))
}

fn parse_effect(value: &str) -> Result<AutomationEffect, HookError<AutomationTriggerHookError>> {
    value
        .parse::<AutomationEffect>()
        .map_err(|err| trigger_unprocessable_error(err.to_string()))
}

fn parse_mutability(
    value: &str,
) -> Result<AutomationRunMutability, HookError<AutomationTriggerHookError>> {
    value
        .parse::<AutomationRunMutability>()
        .map_err(|err| trigger_unprocessable_error(err.to_string()))
}

fn validate_trigger_configuration(
    name: &str,
    activation: AutomationActivation,
    effect: AutomationEffect,
    schedule: &str,
    work_item_selector: Option<&crudkit_core::condition::Condition>,
    prompt: &str,
) -> Result<(), HookError<AutomationTriggerHookError>> {
    automation_triggers::validate_trigger_configuration(
        name,
        activation,
        effect,
        schedule,
        work_item_selector,
        prompt,
    )
    .map_err(|err| trigger_unprocessable_error(err.to_string()))
}

fn normalize_selector_storage(
    activation: AutomationActivation,
    selector: Option<String>,
) -> Result<Option<String>, HookError<AutomationTriggerHookError>> {
    let selector = normalize_optional(selector);
    match (activation, selector) {
        (AutomationActivation::WorkItem, None) => {
            automation_triggers::default_work_item_selector_storage()
                .map(Some)
                .map_err(|err| trigger_internal_error(err.to_string()))
        }
        (_, selector) => {
            let condition = automation_triggers::selector_from_storage(selector.as_deref())
                .map_err(|err| trigger_unprocessable_error(err.to_string()))?;
            automation_triggers::selector_to_storage(condition.as_ref())
                .map_err(|err| trigger_unprocessable_error(err.to_string()))
        }
    }
}

fn parse_work_item_selector(
    selector: Option<&str>,
) -> Result<Option<crudkit_core::condition::Condition>, HookError<AutomationTriggerHookError>> {
    automation_triggers::selector_from_storage(selector)
        .map_err(|err| trigger_unprocessable_error(err.to_string()))
}

async fn default_tool_name_for_project(
    context: &AutomationTriggerResourceContext,
    project_id: i64,
) -> Result<String, HookError<AutomationTriggerHookError>> {
    let project = project::Project::find_by_id(project_id)
        .one(context.store.db().as_ref())
        .await
        .map_err(trigger_internal_error)?
        .ok_or_else(|| {
            trigger_unprocessable_error(format!("project {project_id} does not exist"))
        })?;
    Ok(project.default_agent_tool)
}

async fn trigger_hook_data(
    store: &Store,
    project_id: i64,
    activation: AutomationActivation,
    schedule: &str,
    previous: Option<(AutomationActivation, Option<i64>)>,
) -> Result<AutomationTriggerHookData, HookError<AutomationTriggerHookError>> {
    let next_evaluation_at = match activation {
        AutomationActivation::Manual => None,
        AutomationActivation::WorkItem => None,
        AutomationActivation::Cron => Some(
            automation_triggers::next_evaluation_at(schedule)
                .map_err(|err| trigger_unprocessable_error(err.to_string()))?,
        ),
        AutomationActivation::WorkItemCreated => None,
    };
    let last_event_id = match (previous, activation) {
        (
            Some((AutomationActivation::WorkItemCreated, existing_last_event_id)),
            AutomationActivation::WorkItemCreated,
        ) => existing_last_event_id,
        (_, AutomationActivation::WorkItemCreated) => {
            automation_triggers::latest_item_created_event_id(store, project_id)
                .await
                .map_err(trigger_internal_error)?
        }
        (
            _,
            AutomationActivation::Manual
            | AutomationActivation::WorkItem
            | AutomationActivation::Cron,
        ) => None,
    };

    Ok(AutomationTriggerHookData {
        next_evaluation_at,
        last_event_id,
    })
}

async fn apply_trigger_hook_data(
    context: &AutomationTriggerResourceContext,
    model: &automation_trigger::Model,
    data: AutomationTriggerHookData,
) -> Result<(), HookError<AutomationTriggerHookError>> {
    let mut active: automation_trigger::ActiveModel = model.clone().into();
    active.next_evaluation_at = Set(data.next_evaluation_at);
    active.last_event_id = Set(data.last_event_id);
    active.updated_at = Set(utc_now());
    active
        .update(context.store.db().as_ref())
        .await
        .map_err(trigger_internal_error)?;
    Ok(())
}

fn trigger_unprocessable_error(reason: String) -> HookError<AutomationTriggerHookError> {
    HookError::UnprocessableEntity { reason }
}

fn trigger_internal_error(error: impl fmt::Display) -> HookError<AutomationTriggerHookError> {
    HookError::Internal(AutomationTriggerHookError(error.to_string()))
}

#[derive(Debug, ToSchema)]
pub struct CrudAutomationTriggerResource;

impl CrudResource for CrudAutomationTriggerResource {
    type ReadModel = automation_trigger::read_view::Model;
    type ReadModelId = automation_trigger::read_view::ModelId;
    type ReadModelField = automation_trigger::read_view::ModelField;

    type CreateModel = automation_trigger::CreateModel;
    type CreateModelField = automation_trigger::ModelField;

    type UpdateModel = automation_trigger::UpdateModel;
    type UpdateModelField = automation_trigger::ModelField;

    type Model = automation_trigger::Model;
    type Id = automation_trigger::AutomationTriggerId;
    type ModelField = automation_trigger::ModelField;

    type Repository = SeaOrmRepo;
    type ValidationResultRepository =
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository;
    type CollaborationService = NoopCollaborationService;
    type Context = AutomationTriggerResourceContext;
    type HookData = AutomationTriggerHookData;
    type Lifetime = AutomationTriggerLifetime;
    type Auth = NoAuth;
    type AuthPolicy = OpenAuthPolicy;
    type ResourceType = CrudResources;
    const TYPE: CrudResources = CrudResources::AutomationTrigger;
}

impl SeaOrmResource for CrudAutomationTriggerResource {
    type Entity = automation_trigger::Entity;
    type SeaOrmModel = automation_trigger::Model;
    type ActiveModel = automation_trigger::ActiveModel;
    type Column = automation_trigger::Column;
    type PrimaryKey = <automation_trigger::Entity as EntityTrait>::PrimaryKey;

    type ReadViewEntity = automation_trigger::read_view::Entity;
    type ReadViewSeaOrmModel = automation_trigger::read_view::Model;
    type ReadViewActiveModel = automation_trigger::read_view::ActiveModel;
    type ReadViewColumn = automation_trigger::read_view::Column;
    type ReadViewPrimaryKey = <automation_trigger::read_view::Entity as EntityTrait>::PrimaryKey;

    fn model_field_to_column(field: &Self::ModelField) -> Self::Column {
        <automation_trigger::ModelField as CrudColumns<
            automation_trigger::Column,
        >>::to_sea_orm_column(field)
    }

    fn read_model_field_to_column(field: &Self::ReadModelField) -> Self::ReadViewColumn {
        <automation_trigger::read_view::ModelField as CrudColumns<
            automation_trigger::read_view::Column,
        >>::to_sea_orm_column(field)
    }
}

#[derive(Clone)]
pub struct WorkItemStateResourceContext {
    store: Store,
}

impl fmt::Debug for WorkItemStateResourceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkItemStateResourceContext")
    }
}

impl CrudResourceContext for WorkItemStateResourceContext {}

#[derive(Debug, Clone)]
pub struct WorkItemStateHookError(String);

impl fmt::Display for WorkItemStateHookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WorkItemStateHookError {}

#[derive(Debug)]
pub struct WorkItemStateLifetime;

impl CrudLifetime<CrudWorkItemStateResource> for WorkItemStateLifetime {
    type Error = WorkItemStateHookError;

    async fn before_read(
        _read_request: &mut ReadRequest<CrudWorkItemStateResource>,
        _context: &WorkItemStateResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_read(
        _read_request: &ReadRequest<CrudWorkItemStateResource>,
        _read_result: &mut ReadResult<CrudWorkItemStateResource>,
        _context: &WorkItemStateResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn before_create(
        create_model: &mut work_item_state::CreateModel,
        _context: &WorkItemStateResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        create_model.identifier =
            work_item_states::normalize_identifier(create_model.identifier.clone())
                .map_err(|err| work_item_state_unprocessable_error(err.to_string()))?;
        create_model.name = work_item_states::normalize_name(create_model.name.clone())
            .map_err(|err| work_item_state_unprocessable_error(err.to_string()))?;
        Ok(data)
    }

    async fn after_create(
        _create_model: &work_item_state::CreateModel,
        model: &work_item_state::Model,
        context: &WorkItemStateResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        publish_work_item_state_project_event(&context.store, model.project_id).await;
        Ok(data)
    }

    async fn before_update(
        _existing: &work_item_state::Model,
        update_model: &mut work_item_state::UpdateModel,
        _update_request: &UpdateRequest,
        _context: &WorkItemStateResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        update_model.identifier =
            work_item_states::normalize_identifier(update_model.identifier.clone())
                .map_err(|err| work_item_state_unprocessable_error(err.to_string()))?;
        update_model.name = work_item_states::normalize_name(update_model.name.clone())
            .map_err(|err| work_item_state_unprocessable_error(err.to_string()))?;
        Ok(data)
    }

    async fn after_update(
        _update_model: &work_item_state::UpdateModel,
        model: &work_item_state::Model,
        _update_request: &UpdateRequest,
        context: &WorkItemStateResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        publish_work_item_state_project_event(&context.store, model.project_id).await;
        Ok(data)
    }

    async fn before_delete(
        _model: &work_item_state::Model,
        _delete_request: &DeleteRequest<CrudWorkItemStateResource>,
        _context: &WorkItemStateResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_delete(
        model: &work_item_state::Model,
        _delete_request: &DeleteRequest<CrudWorkItemStateResource>,
        context: &WorkItemStateResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        publish_work_item_state_project_event(&context.store, model.project_id).await;
        Ok(data)
    }
}

fn work_item_state_unprocessable_error(reason: String) -> HookError<WorkItemStateHookError> {
    HookError::UnprocessableEntity { reason }
}

#[derive(Debug, ToSchema)]
pub struct CrudWorkItemStateResource;

impl CrudResource for CrudWorkItemStateResource {
    type ReadModel = work_item_state::read_view::Model;
    type ReadModelId = work_item_state::read_view::ModelId;
    type ReadModelField = work_item_state::read_view::ModelField;

    type CreateModel = work_item_state::CreateModel;
    type CreateModelField = work_item_state::ModelField;

    type UpdateModel = work_item_state::UpdateModel;
    type UpdateModelField = work_item_state::ModelField;

    type Model = work_item_state::Model;
    type Id = work_item_state::WorkItemStateId;
    type ModelField = work_item_state::ModelField;

    type Repository = SeaOrmRepo;
    type ValidationResultRepository =
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository;
    type CollaborationService = NoopCollaborationService;
    type Context = WorkItemStateResourceContext;
    type HookData = ();
    type Lifetime = WorkItemStateLifetime;
    type Auth = NoAuth;
    type AuthPolicy = OpenAuthPolicy;
    type ResourceType = CrudResources;
    const TYPE: CrudResources = CrudResources::WorkItemState;
}

impl SeaOrmResource for CrudWorkItemStateResource {
    type Entity = work_item_state::Entity;
    type SeaOrmModel = work_item_state::Model;
    type ActiveModel = work_item_state::ActiveModel;
    type Column = work_item_state::Column;
    type PrimaryKey = <work_item_state::Entity as EntityTrait>::PrimaryKey;

    type ReadViewEntity = work_item_state::read_view::Entity;
    type ReadViewSeaOrmModel = work_item_state::read_view::Model;
    type ReadViewActiveModel = work_item_state::read_view::ActiveModel;
    type ReadViewColumn = work_item_state::read_view::Column;
    type ReadViewPrimaryKey = <work_item_state::read_view::Entity as EntityTrait>::PrimaryKey;

    fn model_field_to_column(field: &Self::ModelField) -> Self::Column {
        <work_item_state::ModelField as CrudColumns<work_item_state::Column>>::to_sea_orm_column(
            field,
        )
    }

    fn read_model_field_to_column(field: &Self::ReadModelField) -> Self::ReadViewColumn {
        <work_item_state::read_view::ModelField as CrudColumns<
            work_item_state::read_view::Column,
        >>::to_sea_orm_column(field)
    }
}

#[derive(Clone)]
pub struct SwimLaneResourceContext {
    store: Store,
}

impl fmt::Debug for SwimLaneResourceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SwimLaneResourceContext")
    }
}

impl CrudResourceContext for SwimLaneResourceContext {}

#[derive(Debug, Clone)]
pub struct SwimLaneHookError(String);

impl fmt::Display for SwimLaneHookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SwimLaneHookError {}

#[derive(Debug)]
pub struct SwimLaneLifetime;

impl CrudLifetime<CrudSwimLaneResource> for SwimLaneLifetime {
    type Error = SwimLaneHookError;

    async fn before_read(
        _read_request: &mut ReadRequest<CrudSwimLaneResource>,
        _context: &SwimLaneResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_read(
        _read_request: &ReadRequest<CrudSwimLaneResource>,
        _read_result: &mut ReadResult<CrudSwimLaneResource>,
        _context: &SwimLaneResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn before_create(
        create_model: &mut swim_lane::CreateModel,
        _context: &SwimLaneResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        create_model.identifier = swim_lanes::normalize_identifier(create_model.identifier.clone())
            .map_err(|err| swim_lane_unprocessable_error(err.to_string()))?;
        create_model.name = swim_lanes::normalize_name(create_model.name.clone())
            .map_err(|err| swim_lane_unprocessable_error(err.to_string()))?;
        create_model.filter = swim_lanes::normalize_filter_json(create_model.filter.clone())
            .map_err(|err| swim_lane_unprocessable_error(err.to_string()))?;
        create_model.item_order = swim_lanes::normalize_item_order(create_model.item_order.clone())
            .map_err(|err| swim_lane_unprocessable_error(err.to_string()))?;
        Ok(data)
    }

    async fn after_create(
        _create_model: &swim_lane::CreateModel,
        model: &swim_lane::Model,
        context: &SwimLaneResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        publish_swim_lane_project_event(&context.store, model.project_id).await;
        Ok(data)
    }

    async fn before_update(
        _existing: &swim_lane::Model,
        update_model: &mut swim_lane::UpdateModel,
        _update_request: &UpdateRequest,
        _context: &SwimLaneResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        update_model.identifier = swim_lanes::normalize_identifier(update_model.identifier.clone())
            .map_err(|err| swim_lane_unprocessable_error(err.to_string()))?;
        update_model.name = swim_lanes::normalize_name(update_model.name.clone())
            .map_err(|err| swim_lane_unprocessable_error(err.to_string()))?;
        update_model.filter = swim_lanes::normalize_filter_json(update_model.filter.clone())
            .map_err(|err| swim_lane_unprocessable_error(err.to_string()))?;
        update_model.item_order = swim_lanes::normalize_item_order(update_model.item_order.clone())
            .map_err(|err| swim_lane_unprocessable_error(err.to_string()))?;
        Ok(data)
    }

    async fn after_update(
        _update_model: &swim_lane::UpdateModel,
        model: &swim_lane::Model,
        _update_request: &UpdateRequest,
        context: &SwimLaneResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        publish_swim_lane_project_event(&context.store, model.project_id).await;
        Ok(data)
    }

    async fn before_delete(
        _model: &swim_lane::Model,
        _delete_request: &DeleteRequest<CrudSwimLaneResource>,
        _context: &SwimLaneResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        Ok(data)
    }

    async fn after_delete(
        model: &swim_lane::Model,
        _delete_request: &DeleteRequest<CrudSwimLaneResource>,
        context: &SwimLaneResourceContext,
        _request: RequestContext<NoAuth>,
        data: (),
    ) -> Result<(), HookError<Self::Error>> {
        publish_swim_lane_project_event(&context.store, model.project_id).await;
        Ok(data)
    }
}

fn swim_lane_unprocessable_error(reason: String) -> HookError<SwimLaneHookError> {
    HookError::UnprocessableEntity { reason }
}

#[derive(Debug, ToSchema)]
pub struct CrudSwimLaneResource;

impl CrudResource for CrudSwimLaneResource {
    type ReadModel = swim_lane::read_view::Model;
    type ReadModelId = swim_lane::read_view::ModelId;
    type ReadModelField = swim_lane::read_view::ModelField;

    type CreateModel = swim_lane::CreateModel;
    type CreateModelField = swim_lane::ModelField;

    type UpdateModel = swim_lane::UpdateModel;
    type UpdateModelField = swim_lane::ModelField;

    type Model = swim_lane::Model;
    type Id = swim_lane::SwimLaneId;
    type ModelField = swim_lane::ModelField;

    type Repository = SeaOrmRepo;
    type ValidationResultRepository =
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository;
    type CollaborationService = NoopCollaborationService;
    type Context = SwimLaneResourceContext;
    type HookData = ();
    type Lifetime = SwimLaneLifetime;
    type Auth = NoAuth;
    type AuthPolicy = OpenAuthPolicy;
    type ResourceType = CrudResources;
    const TYPE: CrudResources = CrudResources::SwimLane;
}

impl SeaOrmResource for CrudSwimLaneResource {
    type Entity = swim_lane::Entity;
    type SeaOrmModel = swim_lane::Model;
    type ActiveModel = swim_lane::ActiveModel;
    type Column = swim_lane::Column;
    type PrimaryKey = <swim_lane::Entity as EntityTrait>::PrimaryKey;

    type ReadViewEntity = swim_lane::read_view::Entity;
    type ReadViewSeaOrmModel = swim_lane::read_view::Model;
    type ReadViewActiveModel = swim_lane::read_view::ActiveModel;
    type ReadViewColumn = swim_lane::read_view::Column;
    type ReadViewPrimaryKey = <swim_lane::read_view::Entity as EntityTrait>::PrimaryKey;

    fn model_field_to_column(field: &Self::ModelField) -> Self::Column {
        <swim_lane::ModelField as CrudColumns<swim_lane::Column>>::to_sea_orm_column(field)
    }

    fn read_model_field_to_column(field: &Self::ReadModelField) -> Self::ReadViewColumn {
        <swim_lane::read_view::ModelField as CrudColumns<
            swim_lane::read_view::Column,
        >>::to_sea_orm_column(field)
    }
}

#[derive(Clone)]
pub struct CrudContexts {
    pub project: Arc<CrudContext<CrudProjectResource>>,
    pub work_item: Arc<CrudContext<CrudWorkItemResource>>,
    pub comment: Arc<CrudContext<CrudCommentResource>>,
    pub agent_tool: Arc<CrudContext<CrudAgentToolResource>>,
    pub agent_run: Arc<CrudContext<CrudAgentRunResource>>,
    pub automation_trigger: Arc<CrudContext<CrudAutomationTriggerResource>>,
    pub swim_lane: Arc<CrudContext<CrudSwimLaneResource>>,
    pub work_item_state: Arc<CrudContext<CrudWorkItemStateResource>>,
}

pub fn build_contexts(store: Store) -> CrudContexts {
    let db = store.db();
    let repository = Arc::new(SeaOrmRepo::new(db.clone()));
    let validation_result_repository = Arc::new(
        crudkit_sea_orm::validation::unified::repository::UnifiedValidationRepository { db },
    );
    let collab_service = Arc::new(NoopCollaborationService);

    CrudContexts {
        project: Arc::new(CrudContext {
            res_context: Arc::new(ProjectResourceContext {
                store: store.clone(),
            }),
            repository: repository.clone(),
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository: validation_result_repository.clone(),
            collab_service: collab_service.clone(),
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
        work_item: Arc::new(CrudContext {
            res_context: Arc::new(WorkItemResourceContext {
                store: store.clone(),
            }),
            repository: repository.clone(),
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository: validation_result_repository.clone(),
            collab_service: collab_service.clone(),
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
        comment: Arc::new(CrudContext {
            res_context: Arc::new(CommentResourceContext),
            repository: repository.clone(),
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository: validation_result_repository.clone(),
            collab_service: collab_service.clone(),
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
        agent_tool: Arc::new(CrudContext {
            res_context: Arc::new(AgentToolResourceContext),
            repository: repository.clone(),
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository: validation_result_repository.clone(),
            collab_service: collab_service.clone(),
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
        agent_run: Arc::new(CrudContext {
            res_context: Arc::new(AgentRunResourceContext),
            repository: repository.clone(),
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository: validation_result_repository.clone(),
            collab_service: collab_service.clone(),
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
        automation_trigger: Arc::new(CrudContext {
            res_context: Arc::new(AutomationTriggerResourceContext {
                store: store.clone(),
            }),
            repository: repository.clone(),
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository: validation_result_repository.clone(),
            collab_service: collab_service.clone(),
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
        swim_lane: Arc::new(CrudContext {
            res_context: Arc::new(SwimLaneResourceContext {
                store: store.clone(),
            }),
            repository: repository.clone(),
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository: validation_result_repository.clone(),
            collab_service: collab_service.clone(),
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
        work_item_state: Arc::new(CrudContext {
            res_context: Arc::new(WorkItemStateResourceContext { store }),
            repository,
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository,
            collab_service,
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
    }
}
