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
        entities::{agent_run, agent_tool, automation_trigger, comment, project, work_item},
        projects,
        storage::{Store, utc_now},
    },
    shared::view_models::{
        AgentReasoningEffort, AgentToolName, TriggerKind, WorkspaceMode, WorktreeCleanupPolicy,
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
            projects::normalize_optional(create_model.default_agent_model.take());
        projects::validate_agent_model(create_model.default_agent_model.as_deref())
            .map_err(|err| project_unprocessable_error(err.to_string()))?;
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
        if !model.memory.trim().is_empty() {
            projects::snapshot_current_memory_event(
                &context.store,
                &model.name,
                "initial",
                projects::MemoryChangeSource::User,
            )
            .await
            .map_err(|err| ProjectHookError(err.to_string()))
            .map_err(HookError::Internal)?;
        }
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
        projects::validate_settings(
            workspace_mode,
            update_model.max_code_edit_agents,
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
                projects::MemoryChangeSource::User,
            )
            .await
            .map_err(|err| ProjectHookError(err.to_string()))
            .map_err(HookError::Internal)?;
        }
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
        _model: &project::Model,
        _delete_request: &DeleteRequest<CrudProjectResource>,
        _context: &ProjectResourceContext,
        _request: RequestContext<NoAuth>,
        data: ProjectHookData,
    ) -> Result<ProjectHookData, HookError<Self::Error>> {
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

#[derive(Debug, CkResourceContext)]
pub struct WorkItemResourceContext;

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
    type Lifetime = NoopLifetimeHooks;
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
    next_run_at: Option<String>,
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
        create_model.schedule = normalize_optional(create_model.schedule.take());
        let trigger_kind = parse_trigger_kind(&create_model.trigger_kind)?;
        validate_trigger_fields(
            &create_model.name,
            trigger_kind,
            create_model.schedule.as_deref(),
        )?;
        create_model.mode = automation_triggers::default_mode_for_kind(trigger_kind)
            .as_storage()
            .to_owned();
        create_model.tool_name =
            default_tool_name_for_project(context, create_model.project_id).await?;
        trigger_hook_data(
            &context.store,
            create_model.project_id,
            trigger_kind,
            create_model.schedule.as_deref(),
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
        update_model.schedule = normalize_optional(update_model.schedule.take());
        let previous_kind = parse_trigger_kind(&existing.trigger_kind)?;
        let trigger_kind = parse_trigger_kind(&update_model.trigger_kind)?;
        validate_trigger_fields(
            &update_model.name,
            trigger_kind,
            update_model.schedule.as_deref(),
        )?;
        update_model.mode = automation_triggers::default_mode_for_kind(trigger_kind)
            .as_storage()
            .to_owned();
        update_model.tool_name =
            default_tool_name_for_project(context, existing.project_id).await?;
        trigger_hook_data(
            &context.store,
            existing.project_id,
            trigger_kind,
            update_model.schedule.as_deref(),
            Some((previous_kind, existing.last_event_id)),
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
        _model: &automation_trigger::Model,
        _delete_request: &DeleteRequest<CrudAutomationTriggerResource>,
        _context: &AutomationTriggerResourceContext,
        _request: RequestContext<NoAuth>,
        data: AutomationTriggerHookData,
    ) -> Result<AutomationTriggerHookData, HookError<Self::Error>> {
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

fn parse_trigger_kind(value: &str) -> Result<TriggerKind, HookError<AutomationTriggerHookError>> {
    value
        .parse::<TriggerKind>()
        .map_err(|err| trigger_unprocessable_error(err.to_string()))
}

fn validate_trigger_fields(
    name: &str,
    trigger_kind: TriggerKind,
    schedule: Option<&str>,
) -> Result<(), HookError<AutomationTriggerHookError>> {
    automation_triggers::validate_trigger_fields(name, trigger_kind, schedule)
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
    trigger_kind: TriggerKind,
    schedule: Option<&str>,
    previous: Option<(TriggerKind, Option<i64>)>,
) -> Result<AutomationTriggerHookData, HookError<AutomationTriggerHookError>> {
    let next_run_at = match trigger_kind {
        TriggerKind::Cron => Some(
            automation_triggers::next_run_at(schedule.unwrap_or_default())
                .map_err(|err| trigger_unprocessable_error(err.to_string()))?,
        ),
        TriggerKind::WorkItemCreated => None,
    };
    let last_event_id = match (previous, trigger_kind) {
        (
            Some((TriggerKind::WorkItemCreated, existing_last_event_id)),
            TriggerKind::WorkItemCreated,
        ) => existing_last_event_id,
        (_, TriggerKind::WorkItemCreated) => {
            automation_triggers::latest_item_created_event_id(store, project_id)
                .await
                .map_err(trigger_internal_error)?
        }
        (_, TriggerKind::Cron) => None,
    };

    Ok(AutomationTriggerHookData {
        next_run_at,
        last_event_id,
    })
}

async fn apply_trigger_hook_data(
    context: &AutomationTriggerResourceContext,
    model: &automation_trigger::Model,
    data: AutomationTriggerHookData,
) -> Result<(), HookError<AutomationTriggerHookError>> {
    let mut active: automation_trigger::ActiveModel = model.clone().into();
    active.next_run_at = Set(data.next_run_at);
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
pub struct CrudContexts {
    pub project: Arc<CrudContext<CrudProjectResource>>,
    pub work_item: Arc<CrudContext<CrudWorkItemResource>>,
    pub comment: Arc<CrudContext<CrudCommentResource>>,
    pub agent_tool: Arc<CrudContext<CrudAgentToolResource>>,
    pub agent_run: Arc<CrudContext<CrudAgentRunResource>>,
    pub automation_trigger: Arc<CrudContext<CrudAutomationTriggerResource>>,
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
            res_context: Arc::new(WorkItemResourceContext),
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
            res_context: Arc::new(AutomationTriggerResourceContext { store }),
            repository,
            validators: vec![],
            resource_validators: vec![],
            validation_result_repository,
            collab_service,
            global_validation_state: Arc::new(GlobalValidationState::new()),
        }),
    }
}
