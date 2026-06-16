use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;

#[derive(Iden)]
enum CrudkitValidation {
    #[iden = "CrudkitValidation"]
    Table,
    Id,
    ResourceName,
    EntityId,
    ValidatorName,
    ValidatorVersion,
    ViolationSeverity,
    ViolationMessage,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    Name,
    DisplayName,
    Path,
    PathExists,
    PathCheckedAt,
    SystemPrompt,
    Memory,
    WorkspaceMode,
    MaxCodeEditAgents,
    AllowRefinementAgentsDuringEditing,
    CreatePr,
    StaleClaimMinutes,
    WorktreeCleanupPolicy,
    DefaultAgentTool,
    DefaultAgentModel,
    DefaultAgentReasoningEffort,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum WorkItems {
    Table,
    Id,
    ProjectId,
    Title,
    Description,
    State,
    ClaimedBy,
    ClaimedAt,
    ClaimExpiresAt,
    FinishedAt,
    AgentModelOverride,
    AgentReasoningEffortOverride,
    Version,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum WorkItemLabels {
    Table,
    Id,
    ProjectId,
    WorkItemId,
    LabelKey,
    LabelValue,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum SwimLanes {
    Table,
    Id,
    ProjectId,
    Identifier,
    Name,
    Position,
    CanCreateItems,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Comments {
    Table,
    Id,
    WorkItemId,
    AuthorType,
    AuthorName,
    Body,
    CreatedAt,
}

#[derive(DeriveIden)]
enum WorkItemEvents {
    Table,
    Id,
    ProjectId,
    WorkItemId,
    EventType,
    Body,
    ActorType,
    ActorId,
    AgentRunId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum AgentTools {
    Table,
    Id,
    ToolName,
    ExecutablePath,
    DiscoveredPath,
    LastDiscoveredAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum AgentRuns {
    Table,
    Id,
    ProjectId,
    WorkItemId,
    MemoryEventId,
    TriggerId,
    TriggerName,
    Mode,
    ToolName,
    Status,
    Command,
    WorkingDir,
    WorktreePath,
    BranchName,
    ProcessId,
    ExitCode,
    LogPath,
    PromptPath,
    AgentModel,
    AgentReasoningEffort,
    PrRequested,
    PrUrl,
    CleanupStatus,
    WorktreeCleanedAt,
    ResultSummary,
    StartedAt,
    FinishedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum AutomationTriggers {
    Table,
    Id,
    ProjectId,
    Name,
    Enabled,
    Activation,
    Effect,
    Schedule,
    Mode,
    ToolName,
    Prompt,
    WorkItemSelector,
    Priority,
    EvaluationCount,
    PendingEvaluationCount,
    LastEvaluationQueuedAt,
    LastEvaluatedAt,
    NextEvaluationAt,
    LastEventId,
    CreatedAt,
    UpdatedAt,
}

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(CreatePhaseOneTables),
            Box::new(AddPhaseTwoCoordination),
            Box::new(AddProjectContext),
            Box::new(AddPhaseThreeAutomation),
            Box::new(AddPhaseThreeWorkspacePolicy),
            Box::new(AddPhaseFourHardening),
            Box::new(AddProjectDefaultAgentTool),
            Box::new(MoveRunSettingsIntoProjects),
            Box::new(DropClaudeCodeSupport),
            Box::new(RenameProjectRepoPath),
            Box::new(AddProjectPathStatus),
            Box::new(AddAutomationRunConfiguration),
            Box::new(RemoveAutomationTriggerDryRun),
            Box::new(AddAutomationRunTriggerOrigin),
            Box::new(AddProjectMemoryEvents),
            Box::new(RemoveWorkItemAutomationClaimable),
            Box::new(AddLabelsAndSwimLanes),
            Box::new(AddAutomationWorkItemSelectors),
            Box::new(RenameAutomationActivationAndRequireScheduleTransientName),
            Box::new(AddAutomationWorkItemSelectorsTransientName),
            Box::new(RenameAutomationActivationAndRequireSchedule),
            Box::new(AddWorkItemStateLabelReadView),
            Box::new(AddSwimLaneCreateItemFlag),
        ]
    }
}

struct CreatePhaseOneTables;

impl MigrationName for CreatePhaseOneTables {
    fn name(&self) -> &str {
        "migrations"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CreatePhaseOneTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_crudkit_validation(manager).await?;
        create_projects(manager).await?;
        create_work_items(manager).await?;
        create_comments(manager).await?;
        create_work_item_events(manager).await?;
        create_read_views(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_views(manager).await?;
        manager
            .drop_table(Table::drop().table(WorkItemEvents::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Comments::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(WorkItems::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Projects::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(CrudkitValidation::Table).to_owned())
            .await
    }
}

async fn create_crudkit_validation(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(CrudkitValidation::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(CrudkitValidation::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(CrudkitValidation::ResourceName)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(CrudkitValidation::EntityId)
                        .json_binary()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(CrudkitValidation::ValidatorName)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(CrudkitValidation::ValidatorVersion)
                        .big_integer()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(CrudkitValidation::ViolationSeverity)
                        .string_len(16)
                        .not_null(),
                )
                .col(
                    ColumnDef::new(CrudkitValidation::ViolationMessage)
                        .text()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(CrudkitValidation::CreatedAt)
                        .string()
                        .not_null(),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_crudkit_validation_resource_name")
                .table(CrudkitValidation::Table)
                .col(CrudkitValidation::ResourceName)
                .if_not_exists()
                .to_owned(),
        )
        .await
}

async fn create_projects(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Projects::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Projects::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(Projects::Name)
                        .string()
                        .not_null()
                        .unique_key(),
                )
                .col(ColumnDef::new(Projects::DisplayName).string().not_null())
                .col(ColumnDef::new(Projects::Path).string().null())
                .col(
                    ColumnDef::new(Projects::PathExists)
                        .boolean()
                        .not_null()
                        .default(false),
                )
                .col(ColumnDef::new(Projects::PathCheckedAt).string().null())
                .col(
                    ColumnDef::new(Projects::SystemPrompt)
                        .text()
                        .not_null()
                        .default(""),
                )
                .col(
                    ColumnDef::new(Projects::Memory)
                        .text()
                        .not_null()
                        .default(""),
                )
                .col(
                    ColumnDef::new(Projects::WorkspaceMode)
                        .string()
                        .not_null()
                        .default("current_branch"),
                )
                .col(
                    ColumnDef::new(Projects::MaxCodeEditAgents)
                        .big_integer()
                        .not_null()
                        .default(1),
                )
                .col(
                    ColumnDef::new(Projects::AllowRefinementAgentsDuringEditing)
                        .boolean()
                        .not_null()
                        .default(false),
                )
                .col(
                    ColumnDef::new(Projects::CreatePr)
                        .boolean()
                        .not_null()
                        .default(false),
                )
                .col(
                    ColumnDef::new(Projects::StaleClaimMinutes)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    ColumnDef::new(Projects::WorktreeCleanupPolicy)
                        .string()
                        .not_null()
                        .default("manual"),
                )
                .col(
                    ColumnDef::new(Projects::DefaultAgentTool)
                        .string()
                        .not_null()
                        .default("codex"),
                )
                .col(ColumnDef::new(Projects::DefaultAgentModel).string().null())
                .col(
                    ColumnDef::new(Projects::DefaultAgentReasoningEffort)
                        .string()
                        .null(),
                )
                .col(
                    ColumnDef::new(Projects::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(Projects::UpdatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .to_owned(),
        )
        .await
}

async fn create_work_items(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(WorkItems::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(WorkItems::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(WorkItems::ProjectId)
                        .big_integer()
                        .not_null(),
                )
                .col(ColumnDef::new(WorkItems::Title).string().not_null())
                .col(ColumnDef::new(WorkItems::Description).text().not_null())
                .col(
                    ColumnDef::new(WorkItems::State)
                        .string()
                        .not_null()
                        .default("open"),
                )
                .col(ColumnDef::new(WorkItems::ClaimedBy).string().null())
                .col(ColumnDef::new(WorkItems::ClaimedAt).string().null())
                .col(ColumnDef::new(WorkItems::ClaimExpiresAt).string().null())
                .col(ColumnDef::new(WorkItems::FinishedAt).string().null())
                .col(
                    ColumnDef::new(WorkItems::AgentModelOverride)
                        .string()
                        .null(),
                )
                .col(
                    ColumnDef::new(WorkItems::AgentReasoningEffortOverride)
                        .string()
                        .null(),
                )
                .col(
                    ColumnDef::new(WorkItems::Version)
                        .big_integer()
                        .not_null()
                        .default(1),
                )
                .col(
                    ColumnDef::new(WorkItems::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(WorkItems::UpdatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_work_items_project_id")
                        .from(WorkItems::Table, WorkItems::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_work_items_project_state")
                .table(WorkItems::Table)
                .col(WorkItems::ProjectId)
                .col(WorkItems::State)
                .if_not_exists()
                .to_owned(),
        )
        .await
}

struct AddPhaseTwoCoordination;

impl MigrationName for AddPhaseTwoCoordination {
    fn name(&self) -> &str {
        "m20260612_000002_add_phase_two_coordination"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddPhaseTwoCoordination {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column_if_missing(manager, "work_items", "claimed_by", "TEXT").await?;
        add_column_if_missing(manager, "work_items", "claimed_at", "TEXT").await?;
        add_column_if_missing(manager, "work_items", "claim_expires_at", "TEXT").await?;
        add_column_if_missing(manager, "work_items", "finished_at", "TEXT").await?;
        drop_read_view(manager, "work_items_read_view").await?;
        create_read_view(manager, "work_items", "work_items_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "work_items_read_view").await?;
        create_read_view(manager, "work_items", "work_items_read_view").await
    }
}

struct AddProjectContext;

impl MigrationName for AddProjectContext {
    fn name(&self) -> &str {
        "m20260612_000003_add_project_context"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddProjectContext {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column_if_missing(
            manager,
            "projects",
            "system_prompt",
            "TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        add_column_if_missing(manager, "projects", "memory", "TEXT NOT NULL DEFAULT ''").await?;
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await
    }
}

struct AddPhaseThreeAutomation;

impl MigrationName for AddPhaseThreeAutomation {
    fn name(&self) -> &str {
        "m20260612_000004_add_phase_three_automation"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddPhaseThreeAutomation {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_agent_tools(manager).await?;
        create_agent_runs(manager).await?;
        create_automation_triggers(manager).await?;
        create_read_view(manager, "agent_tools", "agent_tools_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await?;
        create_read_view(
            manager,
            "automation_triggers",
            "automation_triggers_read_view",
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "automation_triggers_read_view").await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        drop_read_view(manager, "agent_tools_read_view").await?;
        manager
            .drop_table(Table::drop().table(AutomationTriggers::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AgentRuns::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AgentTools::Table).to_owned())
            .await
    }
}

async fn create_comments(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Comments::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Comments::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(Comments::WorkItemId)
                        .big_integer()
                        .not_null(),
                )
                .col(ColumnDef::new(Comments::AuthorType).string().not_null())
                .col(ColumnDef::new(Comments::AuthorName).string().null())
                .col(ColumnDef::new(Comments::Body).text().not_null())
                .col(
                    ColumnDef::new(Comments::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_comments_work_item_id")
                        .from(Comments::Table, Comments::WorkItemId)
                        .to(WorkItems::Table, WorkItems::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_comments_work_item_created")
                .table(Comments::Table)
                .col(Comments::WorkItemId)
                .col(Comments::CreatedAt)
                .if_not_exists()
                .to_owned(),
        )
        .await
}

async fn create_work_item_events(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(WorkItemEvents::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(WorkItemEvents::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(WorkItemEvents::ProjectId)
                        .big_integer()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(WorkItemEvents::WorkItemId)
                        .big_integer()
                        .null(),
                )
                .col(
                    ColumnDef::new(WorkItemEvents::EventType)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(WorkItemEvents::Body).text().not_null())
                .col(ColumnDef::new(WorkItemEvents::ActorType).string().null())
                .col(ColumnDef::new(WorkItemEvents::ActorId).string().null())
                .col(
                    ColumnDef::new(WorkItemEvents::AgentRunId)
                        .big_integer()
                        .null(),
                )
                .col(
                    ColumnDef::new(WorkItemEvents::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_work_item_events_project_id")
                        .from(WorkItemEvents::Table, WorkItemEvents::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_work_item_events_work_item_id")
                        .from(WorkItemEvents::Table, WorkItemEvents::WorkItemId)
                        .to(WorkItems::Table, WorkItems::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_work_item_events_project_created")
                .table(WorkItemEvents::Table)
                .col(WorkItemEvents::ProjectId)
                .col(WorkItemEvents::CreatedAt)
                .if_not_exists()
                .to_owned(),
        )
        .await
}

async fn create_agent_tools(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AgentTools::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(AgentTools::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(AgentTools::ToolName)
                        .string()
                        .not_null()
                        .unique_key(),
                )
                .col(ColumnDef::new(AgentTools::ExecutablePath).string().null())
                .col(ColumnDef::new(AgentTools::DiscoveredPath).string().null())
                .col(ColumnDef::new(AgentTools::LastDiscoveredAt).string().null())
                .col(
                    ColumnDef::new(AgentTools::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(AgentTools::UpdatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .to_owned(),
        )
        .await
}

async fn create_agent_runs(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AgentRuns::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(AgentRuns::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(AgentRuns::ProjectId)
                        .big_integer()
                        .not_null(),
                )
                .col(ColumnDef::new(AgentRuns::WorkItemId).big_integer().null())
                .col(
                    ColumnDef::new(AgentRuns::MemoryEventId)
                        .big_integer()
                        .null(),
                )
                .col(ColumnDef::new(AgentRuns::TriggerId).big_integer().null())
                .col(ColumnDef::new(AgentRuns::TriggerName).string().null())
                .col(ColumnDef::new(AgentRuns::Mode).string().not_null())
                .col(ColumnDef::new(AgentRuns::ToolName).string().not_null())
                .col(ColumnDef::new(AgentRuns::Status).string().not_null())
                .col(ColumnDef::new(AgentRuns::Command).text().not_null())
                .col(ColumnDef::new(AgentRuns::WorkingDir).string().not_null())
                .col(ColumnDef::new(AgentRuns::WorktreePath).string().null())
                .col(ColumnDef::new(AgentRuns::BranchName).string().null())
                .col(ColumnDef::new(AgentRuns::ProcessId).big_integer().null())
                .col(ColumnDef::new(AgentRuns::ExitCode).big_integer().null())
                .col(ColumnDef::new(AgentRuns::LogPath).string().null())
                .col(ColumnDef::new(AgentRuns::PromptPath).string().null())
                .col(ColumnDef::new(AgentRuns::AgentModel).string().null())
                .col(
                    ColumnDef::new(AgentRuns::AgentReasoningEffort)
                        .string()
                        .null(),
                )
                .col(
                    ColumnDef::new(AgentRuns::PrRequested)
                        .boolean()
                        .not_null()
                        .default(false),
                )
                .col(ColumnDef::new(AgentRuns::PrUrl).string().null())
                .col(
                    ColumnDef::new(AgentRuns::CleanupStatus)
                        .string()
                        .not_null()
                        .default("not_applicable"),
                )
                .col(ColumnDef::new(AgentRuns::WorktreeCleanedAt).string().null())
                .col(
                    ColumnDef::new(AgentRuns::ResultSummary)
                        .text()
                        .not_null()
                        .default(""),
                )
                .col(ColumnDef::new(AgentRuns::StartedAt).string().null())
                .col(ColumnDef::new(AgentRuns::FinishedAt).string().null())
                .col(
                    ColumnDef::new(AgentRuns::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(AgentRuns::UpdatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_agent_runs_project_id")
                        .from(AgentRuns::Table, AgentRuns::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_agent_runs_work_item_id")
                        .from(AgentRuns::Table, AgentRuns::WorkItemId)
                        .to(WorkItems::Table, WorkItems::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_agent_runs_project_status")
                .table(AgentRuns::Table)
                .col(AgentRuns::ProjectId)
                .col(AgentRuns::Status)
                .if_not_exists()
                .to_owned(),
        )
        .await
}

async fn create_automation_triggers(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(AutomationTriggers::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(AutomationTriggers::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::ProjectId)
                        .big_integer()
                        .not_null(),
                )
                .col(ColumnDef::new(AutomationTriggers::Name).string().not_null())
                .col(
                    ColumnDef::new(AutomationTriggers::Enabled)
                        .boolean()
                        .not_null()
                        .default(true),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::Activation)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::Effect)
                        .string()
                        .not_null()
                        .default("consume_work"),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::Schedule)
                        .string()
                        .not_null()
                        .default("@every 15s"),
                )
                .col(ColumnDef::new(AutomationTriggers::Mode).string().not_null())
                .col(
                    ColumnDef::new(AutomationTriggers::ToolName)
                        .string()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::Prompt)
                        .text()
                        .not_null()
                        .default(""),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::WorkItemSelector)
                        .text()
                        .null(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::Priority)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::EvaluationCount)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::PendingEvaluationCount)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::LastEvaluationQueuedAt)
                        .string()
                        .null(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::LastEvaluatedAt)
                        .string()
                        .null(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::NextEvaluationAt)
                        .string()
                        .null(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::LastEventId)
                        .big_integer()
                        .null(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::UpdatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_automation_triggers_project_id")
                        .from(AutomationTriggers::Table, AutomationTriggers::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_automation_triggers_project_activation")
                .table(AutomationTriggers::Table)
                .col(AutomationTriggers::ProjectId)
                .col(AutomationTriggers::Activation)
                .if_not_exists()
                .to_owned(),
        )
        .await
}

struct AddPhaseThreeWorkspacePolicy;

impl MigrationName for AddPhaseThreeWorkspacePolicy {
    fn name(&self) -> &str {
        "m20260612_000005_add_phase_three_workspace_policy"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddPhaseThreeWorkspacePolicy {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_project_run_settings_columns(manager).await?;
        add_column_if_missing(
            manager,
            "agent_runs",
            "pr_requested",
            "BOOLEAN NOT NULL DEFAULT 0",
        )
        .await?;
        add_column_if_missing(manager, "agent_runs", "pr_url", "TEXT").await?;
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }
}

struct AddPhaseFourHardening;

impl MigrationName for AddPhaseFourHardening {
    fn name(&self) -> &str {
        "m20260612_000006_add_phase_four_hardening"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddPhaseFourHardening {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_project_run_settings_columns(manager).await?;
        add_column_if_missing(
            manager,
            "agent_runs",
            "cleanup_status",
            "TEXT NOT NULL DEFAULT 'not_applicable'",
        )
        .await?;
        add_column_if_missing(manager, "agent_runs", "worktree_cleaned_at", "TEXT").await?;
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }
}

struct AddProjectDefaultAgentTool;

impl MigrationName for AddProjectDefaultAgentTool {
    fn name(&self) -> &str {
        "m20260612_000007_add_project_default_agent_tool"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddProjectDefaultAgentTool {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_project_run_settings_columns(manager).await?;
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await
    }
}

struct MoveRunSettingsIntoProjects;

impl MigrationName for MoveRunSettingsIntoProjects {
    fn name(&self) -> &str {
        "m20260613_000008_move_run_settings_into_projects"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for MoveRunSettingsIntoProjects {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_project_run_settings_columns(manager).await?;
        if table_exists(manager, "project_settings").await? {
            manager
                .get_connection()
                .execute(Statement::from_string(
                    manager.get_database_backend(),
                    r#"
                    UPDATE "projects"
                    SET
                        "workspace_mode" = COALESCE((
                            SELECT "workspace_mode"
                            FROM "project_settings"
                            WHERE "project_settings"."project_id" = "projects"."id"
                        ), "workspace_mode"),
                        "max_code_edit_agents" = COALESCE((
                            SELECT "max_code_edit_agents"
                            FROM "project_settings"
                            WHERE "project_settings"."project_id" = "projects"."id"
                        ), "max_code_edit_agents"),
                        "allow_refinement_agents_during_editing" = COALESCE((
                            SELECT "allow_refinement_agents_during_editing"
                            FROM "project_settings"
                            WHERE "project_settings"."project_id" = "projects"."id"
                        ), "allow_refinement_agents_during_editing"),
                        "create_pr" = COALESCE((
                            SELECT "create_pr"
                            FROM "project_settings"
                            WHERE "project_settings"."project_id" = "projects"."id"
                        ), "create_pr"),
                        "stale_claim_minutes" = COALESCE((
                            SELECT "stale_claim_minutes"
                            FROM "project_settings"
                            WHERE "project_settings"."project_id" = "projects"."id"
                        ), "stale_claim_minutes"),
                        "worktree_cleanup_policy" = COALESCE((
                            SELECT "worktree_cleanup_policy"
                            FROM "project_settings"
                            WHERE "project_settings"."project_id" = "projects"."id"
                        ), "worktree_cleanup_policy"),
                        "default_agent_tool" = COALESCE((
                            SELECT "default_agent_tool"
                            FROM "project_settings"
                            WHERE "project_settings"."project_id" = "projects"."id"
                        ), "default_agent_tool")
                    WHERE EXISTS (
                        SELECT 1
                        FROM "project_settings"
                        WHERE "project_settings"."project_id" = "projects"."id"
                    );
                    "#,
                ))
                .await?;
            drop_read_view(manager, "project_settings_read_view").await?;
            manager
                .get_connection()
                .execute(Statement::from_string(
                    manager.get_database_backend(),
                    r#"DROP TABLE IF EXISTS "project_settings";"#,
                ))
                .await?;
        }
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await
    }
}

struct DropClaudeCodeSupport;

impl MigrationName for DropClaudeCodeSupport {
    fn name(&self) -> &str {
        "m20260613_000009_drop_claude_code_support"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for DropClaudeCodeSupport {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        for statement in [
            r#"UPDATE "projects" SET "default_agent_tool" = 'codex' WHERE "default_agent_tool" != 'codex';"#,
            r#"UPDATE "agent_runs" SET "tool_name" = 'codex' WHERE "tool_name" != 'codex';"#,
            r#"UPDATE "automation_triggers" SET "tool_name" = 'codex' WHERE "tool_name" != 'codex';"#,
            r#"DELETE FROM "agent_tools" WHERE "tool_name" != 'codex';"#,
        ] {
            manager
                .get_connection()
                .execute(Statement::from_string(backend, statement))
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

struct RenameProjectRepoPath;

impl MigrationName for RenameProjectRepoPath {
    fn name(&self) -> &str {
        "m20260613_000010_rename_project_repo_path"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RenameProjectRepoPath {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rename_project_path_column(manager, "repo_path", "path").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rename_project_path_column(manager, "path", "repo_path").await
    }
}

struct AddProjectPathStatus;

impl MigrationName for AddProjectPathStatus {
    fn name(&self) -> &str {
        "m20260613_000011_add_project_path_status"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddProjectPathStatus {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_project_path_status_columns(manager).await?;
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await
    }
}

struct AddAutomationRunConfiguration;

impl MigrationName for AddAutomationRunConfiguration {
    fn name(&self) -> &str {
        "m20260613_000012_add_automation_run_configuration"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddAutomationRunConfiguration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column_if_missing(manager, "projects", "default_agent_model", "TEXT").await?;
        add_column_if_missing(
            manager,
            "projects",
            "default_agent_reasoning_effort",
            "TEXT",
        )
        .await?;
        add_column_if_missing(
            manager,
            "work_items",
            "automation_claimable",
            "BOOLEAN NOT NULL DEFAULT 1",
        )
        .await?;
        add_column_if_missing(manager, "work_items", "agent_model_override", "TEXT").await?;
        add_column_if_missing(
            manager,
            "work_items",
            "agent_reasoning_effort_override",
            "TEXT",
        )
        .await?;
        add_column_if_missing(manager, "agent_runs", "agent_model", "TEXT").await?;
        add_column_if_missing(manager, "agent_runs", "agent_reasoning_effort", "TEXT").await?;
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await?;
        drop_read_view(manager, "work_items_read_view").await?;
        create_read_view(manager, "work_items", "work_items_read_view").await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "projects_read_view").await?;
        create_read_view(manager, "projects", "projects_read_view").await?;
        drop_read_view(manager, "work_items_read_view").await?;
        create_read_view(manager, "work_items", "work_items_read_view").await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }
}

struct RemoveAutomationTriggerDryRun;

impl MigrationName for RemoveAutomationTriggerDryRun {
    fn name(&self) -> &str {
        "m20260614_000013_remove_automation_trigger_dry_run"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RemoveAutomationTriggerDryRun {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "automation_triggers_read_view").await?;
        drop_column_if_present(manager, "automation_triggers", "dry_run").await?;
        create_read_view(
            manager,
            "automation_triggers",
            "automation_triggers_read_view",
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "automation_triggers_read_view").await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "dry_run",
            "BOOLEAN NOT NULL DEFAULT 0",
        )
        .await?;
        create_read_view(
            manager,
            "automation_triggers",
            "automation_triggers_read_view",
        )
        .await
    }
}

struct AddAutomationRunTriggerOrigin;

impl MigrationName for AddAutomationRunTriggerOrigin {
    fn name(&self) -> &str {
        "m20260614_000014_add_automation_run_trigger_origin"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddAutomationRunTriggerOrigin {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column_if_missing(manager, "agent_runs", "trigger_id", "BIGINT").await?;
        add_column_if_missing(manager, "agent_runs", "trigger_name", "TEXT").await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_agent_runs_trigger_id")
                    .table(AgentRuns::Table)
                    .col(AgentRuns::TriggerId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "agent_runs_read_view").await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_agent_runs_trigger_id")
                    .table(AgentRuns::Table)
                    .to_owned(),
            )
            .await?;
        drop_column_if_present(manager, "agent_runs", "trigger_name").await?;
        drop_column_if_present(manager, "agent_runs", "trigger_id").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }
}

struct AddProjectMemoryEvents;

impl MigrationName for AddProjectMemoryEvents {
    fn name(&self) -> &str {
        "m20260614_000015_add_project_memory_events"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddProjectMemoryEvents {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        add_column_if_missing(manager, "work_item_events", "actor_type", "TEXT").await?;
        add_column_if_missing(manager, "work_item_events", "actor_id", "TEXT").await?;
        add_column_if_missing(manager, "work_item_events", "agent_run_id", "BIGINT").await?;
        add_column_if_missing(manager, "agent_runs", "memory_event_id", "BIGINT").await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_work_item_events_project_type_id")
                    .table(WorkItemEvents::Table)
                    .col(WorkItemEvents::ProjectId)
                    .col(WorkItemEvents::EventType)
                    .col(WorkItemEvents::Id)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        drop_read_view(manager, "agent_runs_read_view").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "agent_runs_read_view").await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_work_item_events_project_type_id")
                    .table(WorkItemEvents::Table)
                    .to_owned(),
            )
            .await?;
        drop_column_if_present(manager, "agent_runs", "memory_event_id").await?;
        drop_column_if_present(manager, "work_item_events", "agent_run_id").await?;
        drop_column_if_present(manager, "work_item_events", "actor_id").await?;
        drop_column_if_present(manager, "work_item_events", "actor_type").await?;
        create_read_view(manager, "agent_runs", "agent_runs_read_view").await
    }
}

struct RemoveWorkItemAutomationClaimable;

impl MigrationName for RemoveWorkItemAutomationClaimable {
    fn name(&self) -> &str {
        "m20260615_000016_remove_work_item_automation_claimable"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RemoveWorkItemAutomationClaimable {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "work_items_read_view").await?;
        if column_exists(manager, "work_items", "automation_claimable").await? {
            manager
                .get_connection()
                .execute(Statement::from_string(
                    manager.get_database_backend(),
                    r#"
                    UPDATE "work_items"
                    SET "state" = 'idea'
                    WHERE "state" = 'open'
                      AND "automation_claimable" = 0;
                    "#,
                ))
                .await?;
        }
        drop_column_if_present(manager, "work_items", "automation_claimable").await?;
        create_read_view(manager, "work_items", "work_items_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "work_items_read_view").await?;
        add_column_if_missing(
            manager,
            "work_items",
            "automation_claimable",
            "BOOLEAN NOT NULL DEFAULT 1",
        )
        .await?;
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                r#"
                UPDATE "work_items"
                SET
                    "automation_claimable" = 0,
                    "state" = 'open'
                WHERE "state" = 'idea';
                "#,
            ))
            .await?;
        create_read_view(manager, "work_items", "work_items_read_view").await
    }
}

struct AddLabelsAndSwimLanes;

impl MigrationName for AddLabelsAndSwimLanes {
    fn name(&self) -> &str {
        "m20260615_000017_add_labels_and_swim_lanes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddLabelsAndSwimLanes {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "work_items_read_view").await?;
        create_work_item_labels(manager).await?;
        create_swim_lanes(manager).await?;
        migrate_work_item_state_to_labels(manager).await?;
        drop_index_if_present(manager, "idx_work_items_project_state").await?;
        drop_column_if_present(manager, "work_items", "state").await?;
        create_work_items_read_view(manager).await?;
        create_read_view(manager, "work_item_labels", "work_item_labels_read_view").await?;
        create_read_view(manager, "swim_lanes", "swim_lanes_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "swim_lanes_read_view").await?;
        drop_read_view(manager, "work_item_labels_read_view").await?;
        drop_read_view(manager, "work_items_read_view").await?;
        add_column_if_missing(
            manager,
            "work_items",
            "state",
            "TEXT NOT NULL DEFAULT 'open'",
        )
        .await?;
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                r#"
                UPDATE "work_items"
                SET "state" = COALESCE((
                    SELECT "label_value"
                    FROM "work_item_labels"
                    WHERE "work_item_labels"."work_item_id" = "work_items"."id"
                      AND "label_key" = 'state'
                    LIMIT 1
                ), 'open');
                "#,
            ))
            .await?;
        manager
            .drop_table(Table::drop().table(SwimLanes::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(WorkItemLabels::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_work_items_project_state")
                    .table(WorkItems::Table)
                    .col(WorkItems::ProjectId)
                    .col(WorkItems::State)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        create_read_view(manager, "work_items", "work_items_read_view").await
    }
}

struct AddAutomationWorkItemSelectors;

impl MigrationName for AddAutomationWorkItemSelectors {
    fn name(&self) -> &str {
        "m20260615_000018_add_automation_work_item_selectors"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddAutomationWorkItemSelectors {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "automation_triggers_read_view").await?;
        add_column_if_missing(manager, "automation_triggers", "work_item_selector", "TEXT").await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "priority",
            "BIGINT NOT NULL DEFAULT 0",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "evaluation_count",
            "BIGINT NOT NULL DEFAULT 0",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "effect",
            "TEXT NOT NULL DEFAULT 'consume_work'",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "pending_evaluation_count",
            "BIGINT NOT NULL DEFAULT 0",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "last_evaluation_queued_at",
            "TEXT",
        )
        .await?;
        seed_default_work_item_automations(manager).await?;
        create_read_view(
            manager,
            "automation_triggers",
            "automation_triggers_read_view",
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "automation_triggers_read_view").await?;
        drop_column_if_present(manager, "automation_triggers", "last_evaluation_queued_at").await?;
        drop_column_if_present(manager, "automation_triggers", "pending_evaluation_count").await?;
        drop_column_if_present(manager, "automation_triggers", "effect").await?;
        drop_column_if_present(manager, "automation_triggers", "evaluation_count").await?;
        drop_column_if_present(manager, "automation_triggers", "priority").await?;
        drop_column_if_present(manager, "automation_triggers", "work_item_selector").await?;
        create_read_view(
            manager,
            "automation_triggers",
            "automation_triggers_read_view",
        )
        .await
    }
}

struct RenameAutomationActivationAndRequireScheduleTransientName;

impl MigrationName for RenameAutomationActivationAndRequireScheduleTransientName {
    fn name(&self) -> &str {
        "m20260615_000018_rename_automation_activation_require_schedule"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RenameAutomationActivationAndRequireScheduleTransientName {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

struct AddAutomationWorkItemSelectorsTransientName;

impl MigrationName for AddAutomationWorkItemSelectorsTransientName {
    fn name(&self) -> &str {
        "m20260615_000019_add_automation_work_item_selectors"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddAutomationWorkItemSelectorsTransientName {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

struct RenameAutomationActivationAndRequireSchedule;

impl MigrationName for RenameAutomationActivationAndRequireSchedule {
    fn name(&self) -> &str {
        "m20260615_000020_rename_automation_activation_require_schedule"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RenameAutomationActivationAndRequireSchedule {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "automation_triggers_read_view").await?;
        rename_column_if_present(manager, "automation_triggers", "trigger_kind", "activation")
            .await?;
        rename_column_if_present(
            manager,
            "automation_triggers",
            "run_count",
            "evaluation_count",
        )
        .await?;
        rename_column_if_present(
            manager,
            "automation_triggers",
            "scheduled_run_count",
            "pending_evaluation_count",
        )
        .await?;
        rename_column_if_present(
            manager,
            "automation_triggers",
            "last_scheduled_run_at",
            "last_evaluation_queued_at",
        )
        .await?;
        rename_column_if_present(
            manager,
            "automation_triggers",
            "last_run_at",
            "last_evaluated_at",
        )
        .await?;
        rename_column_if_present(
            manager,
            "automation_triggers",
            "next_run_at",
            "next_evaluation_at",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "activation",
            "TEXT NOT NULL DEFAULT 'work_item'",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "effect",
            "TEXT NOT NULL DEFAULT 'consume_work'",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "schedule",
            "TEXT NOT NULL DEFAULT '@every 15s'",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "evaluation_count",
            "BIGINT NOT NULL DEFAULT 0",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "pending_evaluation_count",
            "BIGINT NOT NULL DEFAULT 0",
        )
        .await?;
        add_column_if_missing(
            manager,
            "automation_triggers",
            "last_evaluation_queued_at",
            "TEXT",
        )
        .await?;
        add_column_if_missing(manager, "automation_triggers", "last_evaluated_at", "TEXT").await?;
        add_column_if_missing(manager, "automation_triggers", "next_evaluation_at", "TEXT").await?;
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                r#"
                UPDATE "automation_triggers"
                SET
                    "activation" = CASE
                        WHEN "activation" = 'manual' THEN 'work_item'
                        ELSE COALESCE(NULLIF("activation", ''), 'work_item')
                    END,
                    "effect" = COALESCE(NULLIF("effect", ''), 'consume_work'),
                    "schedule" = COALESCE(NULLIF("schedule", ''), '@every 15s'),
                    "evaluation_count" = COALESCE("evaluation_count", 0),
                    "pending_evaluation_count" = COALESCE("pending_evaluation_count", 0);
                "#,
            ))
            .await?;
        drop_index_if_present(manager, "idx_automation_triggers_project_kind").await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_automation_triggers_project_activation")
                    .table(AutomationTriggers::Table)
                    .col(AutomationTriggers::ProjectId)
                    .col(AutomationTriggers::Activation)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        create_read_view(
            manager,
            "automation_triggers",
            "automation_triggers_read_view",
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "automation_triggers_read_view").await?;
        drop_index_if_present(manager, "idx_automation_triggers_project_activation").await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_automation_triggers_project_kind")
                    .table(AutomationTriggers::Table)
                    .col(AutomationTriggers::ProjectId)
                    .col(AutomationTriggers::Activation)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        drop_column_if_present(manager, "automation_triggers", "last_evaluation_queued_at").await?;
        drop_column_if_present(manager, "automation_triggers", "pending_evaluation_count").await?;
        drop_column_if_present(manager, "automation_triggers", "effect").await?;
        rename_column_if_present(manager, "automation_triggers", "activation", "trigger_kind")
            .await?;
        create_read_view(
            manager,
            "automation_triggers",
            "automation_triggers_read_view",
        )
        .await
    }
}

struct AddWorkItemStateLabelReadView;

impl MigrationName for AddWorkItemStateLabelReadView {
    fn name(&self) -> &str {
        "m20260615_000021_add_work_item_state_label_read_view"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddWorkItemStateLabelReadView {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_work_items_read_view(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "work_items_read_view").await?;
        create_read_view(manager, "work_items", "work_items_read_view").await
    }
}

struct AddSwimLaneCreateItemFlag;

impl MigrationName for AddSwimLaneCreateItemFlag {
    fn name(&self) -> &str {
        "m20260616_000022_add_swim_lane_create_item_flag"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddSwimLaneCreateItemFlag {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "swim_lanes_read_view").await?;
        add_column_if_missing(
            manager,
            "swim_lanes",
            "can_create_items",
            "BOOLEAN NOT NULL DEFAULT 0",
        )
        .await?;
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                r#"
                UPDATE "swim_lanes"
                SET "can_create_items" = 1
                WHERE "identifier" IN ('idea', 'open');
                "#
                .to_owned(),
            ))
            .await?;
        create_read_view(manager, "swim_lanes", "swim_lanes_read_view").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_read_view(manager, "swim_lanes_read_view").await?;
        drop_column_if_present(manager, "swim_lanes", "can_create_items").await?;
        create_read_view(manager, "swim_lanes", "swim_lanes_read_view").await
    }
}

async fn seed_default_work_item_automations(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            r#"
            INSERT INTO "automation_triggers"
                (
                    "project_id",
                    "name",
                    "enabled",
                    "activation",
                    "effect",
                    "schedule",
                    "mode",
                    "tool_name",
                    "prompt",
                    "work_item_selector",
                    "priority",
                    "evaluation_count",
                    "pending_evaluation_count",
                    "last_evaluation_queued_at",
                    "last_evaluated_at",
                    "next_evaluation_at",
                    "last_event_id",
                    "created_at",
                    "updated_at"
                )
            SELECT
                "projects"."id",
                'Claim open work',
                1,
                'work_item',
                'consume_work',
                '@every 15s',
                'execute',
                COALESCE(NULLIF("projects"."default_agent_tool", ''), 'codex'),
                '',
                '{"All":[{"column_name":"state","operator":"=","value":{"String":"open"}}]}',
                0,
                0,
                0,
                NULL,
                NULL,
                NULL,
                NULL,
                CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP
            FROM "projects"
            WHERE NOT EXISTS (
                SELECT 1
                FROM "automation_triggers"
                WHERE "automation_triggers"."project_id" = "projects"."id"
                  AND "automation_triggers"."activation" IN ('work_item', 'manual')
            );
            "#,
        ))
        .await
        .map(|_| ())
}

async fn create_work_item_labels(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(WorkItemLabels::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(WorkItemLabels::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(WorkItemLabels::ProjectId)
                        .big_integer()
                        .not_null(),
                )
                .col(
                    ColumnDef::new(WorkItemLabels::WorkItemId)
                        .big_integer()
                        .not_null(),
                )
                .col(ColumnDef::new(WorkItemLabels::LabelKey).string().not_null())
                .col(ColumnDef::new(WorkItemLabels::LabelValue).string().null())
                .col(
                    ColumnDef::new(WorkItemLabels::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(WorkItemLabels::UpdatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_work_item_labels_project_id")
                        .from(WorkItemLabels::Table, WorkItemLabels::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_work_item_labels_work_item_id")
                        .from(WorkItemLabels::Table, WorkItemLabels::WorkItemId)
                        .to(WorkItems::Table, WorkItems::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_work_item_labels_project_key_value")
                .table(WorkItemLabels::Table)
                .col(WorkItemLabels::ProjectId)
                .col(WorkItemLabels::LabelKey)
                .col(WorkItemLabels::LabelValue)
                .if_not_exists()
                .to_owned(),
        )
        .await?;
    manager
        .create_index(
            Index::create()
                .name("idx_work_item_labels_unique_item_key")
                .table(WorkItemLabels::Table)
                .col(WorkItemLabels::WorkItemId)
                .col(WorkItemLabels::LabelKey)
                .unique()
                .if_not_exists()
                .to_owned(),
        )
        .await
}

async fn create_swim_lanes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(SwimLanes::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(SwimLanes::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(
                    ColumnDef::new(SwimLanes::ProjectId)
                        .big_integer()
                        .not_null(),
                )
                .col(ColumnDef::new(SwimLanes::Identifier).string().not_null())
                .col(ColumnDef::new(SwimLanes::Name).string().not_null())
                .col(
                    ColumnDef::new(SwimLanes::Position)
                        .big_integer()
                        .not_null()
                        .default(0),
                )
                .col(
                    ColumnDef::new(SwimLanes::CanCreateItems)
                        .boolean()
                        .not_null()
                        .default(false),
                )
                .col(
                    ColumnDef::new(SwimLanes::CreatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .col(
                    ColumnDef::new(SwimLanes::UpdatedAt)
                        .string()
                        .not_null()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_swim_lanes_project_id")
                        .from(SwimLanes::Table, SwimLanes::ProjectId)
                        .to(Projects::Table, Projects::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

    manager
        .create_index(
            Index::create()
                .name("idx_swim_lanes_unique_project_identifier")
                .table(SwimLanes::Table)
                .col(SwimLanes::ProjectId)
                .col(SwimLanes::Identifier)
                .unique()
                .if_not_exists()
                .to_owned(),
        )
        .await
}

async fn migrate_work_item_state_to_labels(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let backend = manager.get_database_backend();
    let conn = manager.get_connection();
    conn.execute(Statement::from_string(
        backend,
        r#"
        INSERT OR IGNORE INTO "work_item_labels"
            ("project_id", "work_item_id", "label_key", "label_value", "created_at", "updated_at")
        SELECT
            "project_id",
            "id",
            'state',
            COALESCE(NULLIF("state", ''), 'open'),
            COALESCE("created_at", CURRENT_TIMESTAMP),
            COALESCE("updated_at", CURRENT_TIMESTAMP)
        FROM "work_items"
        WHERE "state" IS NOT NULL;
        "#,
    ))
    .await?;

    conn.execute(Statement::from_string(
        backend,
        r#"
        INSERT OR IGNORE INTO "swim_lanes"
            ("project_id", "identifier", "name", "position", "can_create_items", "created_at", "updated_at")
        SELECT "id", 'idea', 'Idea', 10, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
        FROM "projects";
        "#,
    ))
    .await?;
    conn.execute(Statement::from_string(
        backend,
        r#"
        INSERT OR IGNORE INTO "swim_lanes"
            ("project_id", "identifier", "name", "position", "can_create_items", "created_at", "updated_at")
        SELECT "id", 'open', 'Open', 20, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
        FROM "projects";
        "#,
    ))
    .await?;
    conn.execute(Statement::from_string(
        backend,
        r#"
        INSERT OR IGNORE INTO "swim_lanes"
            ("project_id", "identifier", "name", "position", "can_create_items", "created_at", "updated_at")
        SELECT "id", 'in_progress', 'In progress', 30, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
        FROM "projects";
        "#,
    ))
    .await?;
    conn.execute(Statement::from_string(
        backend,
        r#"
        INSERT OR IGNORE INTO "swim_lanes"
            ("project_id", "identifier", "name", "position", "can_create_items", "created_at", "updated_at")
        SELECT "id", 'done', 'Done', 40, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
        FROM "projects";
        "#,
    ))
    .await?;
    Ok(())
}

async fn create_read_views(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    create_read_view(manager, "projects", "projects_read_view").await?;
    create_read_view(manager, "work_items", "work_items_read_view").await?;
    create_read_view(manager, "comments", "comments_read_view").await
}

async fn drop_read_views(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    drop_read_view(manager, "comments_read_view").await?;
    drop_read_view(manager, "work_items_read_view").await?;
    drop_read_view(manager, "projects_read_view").await
}

async fn create_read_view(
    manager: &SchemaManager<'_>,
    table_name: &str,
    view_name: &str,
) -> Result<(), DbErr> {
    drop_read_view(manager, view_name).await?;
    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            format!(
                r#"
                CREATE VIEW "{view_name}" AS
                SELECT "{table_name}".*, 0 AS has_validation_errors
                FROM "{table_name}";
                "#
            ),
        ))
        .await
        .map(|_| ())
}

async fn create_work_items_read_view(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    drop_read_view(manager, "work_items_read_view").await?;
    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            r#"
            CREATE VIEW "work_items_read_view" AS
            SELECT
                "work_items".*,
                (
                    SELECT "work_item_labels"."label_value"
                    FROM "work_item_labels"
                    WHERE "work_item_labels"."project_id" = "work_items"."project_id"
                      AND "work_item_labels"."work_item_id" = "work_items"."id"
                      AND "work_item_labels"."label_key" = 'state'
                    LIMIT 1
                ) AS "state_label",
                0 AS "has_validation_errors"
            FROM "work_items";
            "#,
        ))
        .await
        .map(|_| ())
}

async fn drop_read_view(manager: &SchemaManager<'_>, view_name: &str) -> Result<(), DbErr> {
    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            format!(r#"DROP VIEW IF EXISTS "{view_name}";"#),
        ))
        .await
        .map(|_| ())
}

async fn rename_project_path_column(
    manager: &SchemaManager<'_>,
    from: &str,
    to: &str,
) -> Result<(), DbErr> {
    drop_read_view(manager, "projects_read_view").await?;
    if column_exists(manager, "projects", to).await? {
        if column_exists(manager, "projects", from).await? {
            manager
                .get_connection()
                .execute(Statement::from_string(
                    manager.get_database_backend(),
                    format!(
                        r#"
                        UPDATE "projects"
                        SET "{to}" = COALESCE(NULLIF("{to}", ''), "{from}")
                        WHERE "{from}" IS NOT NULL;
                        "#
                    ),
                ))
                .await?;
        }
    } else if column_exists(manager, "projects", from).await? {
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                format!(r#"ALTER TABLE "projects" RENAME COLUMN "{from}" TO "{to}";"#),
            ))
            .await?;
    } else {
        add_column_if_missing(manager, "projects", to, "TEXT").await?;
    }
    create_read_view(manager, "projects", "projects_read_view").await
}

async fn add_project_run_settings_columns(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    add_column_if_missing(
        manager,
        "projects",
        "workspace_mode",
        "TEXT NOT NULL DEFAULT 'current_branch'",
    )
    .await?;
    add_column_if_missing(
        manager,
        "projects",
        "max_code_edit_agents",
        "BIGINT NOT NULL DEFAULT 1",
    )
    .await?;
    add_column_if_missing(
        manager,
        "projects",
        "allow_refinement_agents_during_editing",
        "BOOLEAN NOT NULL DEFAULT 0",
    )
    .await?;
    add_column_if_missing(
        manager,
        "projects",
        "create_pr",
        "BOOLEAN NOT NULL DEFAULT 0",
    )
    .await?;
    add_column_if_missing(
        manager,
        "projects",
        "stale_claim_minutes",
        "BIGINT NOT NULL DEFAULT 0",
    )
    .await?;
    add_column_if_missing(
        manager,
        "projects",
        "worktree_cleanup_policy",
        "TEXT NOT NULL DEFAULT 'manual'",
    )
    .await?;
    add_column_if_missing(
        manager,
        "projects",
        "default_agent_tool",
        "TEXT NOT NULL DEFAULT 'codex'",
    )
    .await?;
    add_column_if_missing(manager, "projects", "default_agent_model", "TEXT").await?;
    add_column_if_missing(
        manager,
        "projects",
        "default_agent_reasoning_effort",
        "TEXT",
    )
    .await
}

async fn add_project_path_status_columns(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    add_column_if_missing(
        manager,
        "projects",
        "path_exists",
        "BOOLEAN NOT NULL DEFAULT 0",
    )
    .await?;
    add_column_if_missing(manager, "projects", "path_checked_at", "TEXT").await
}

async fn add_column_if_missing(
    manager: &SchemaManager<'_>,
    table_name: &str,
    column_name: &str,
    column_type: &str,
) -> Result<(), DbErr> {
    if column_exists(manager, table_name, column_name).await? {
        return Ok(());
    }

    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            format!("ALTER TABLE \"{table_name}\" ADD COLUMN \"{column_name}\" {column_type};"),
        ))
        .await
        .map(|_| ())
}

async fn drop_column_if_present(
    manager: &SchemaManager<'_>,
    table_name: &str,
    column_name: &str,
) -> Result<(), DbErr> {
    if !column_exists(manager, table_name, column_name).await? {
        return Ok(());
    }

    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            format!(r#"ALTER TABLE "{table_name}" DROP COLUMN "{column_name}";"#),
        ))
        .await
        .map(|_| ())
}

async fn rename_column_if_present(
    manager: &SchemaManager<'_>,
    table_name: &str,
    from: &str,
    to: &str,
) -> Result<(), DbErr> {
    if column_exists(manager, table_name, to).await? {
        return Ok(());
    }
    if !column_exists(manager, table_name, from).await? {
        return Ok(());
    }

    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            format!(r#"ALTER TABLE "{table_name}" RENAME COLUMN "{from}" TO "{to}";"#),
        ))
        .await
        .map(|_| ())
}

async fn drop_index_if_present(manager: &SchemaManager<'_>, index_name: &str) -> Result<(), DbErr> {
    if !index_exists(manager, index_name).await? {
        return Ok(());
    }

    manager
        .get_connection()
        .execute(Statement::from_string(
            manager.get_database_backend(),
            format!(r#"DROP INDEX "{index_name}";"#),
        ))
        .await
        .map(|_| ())
}

async fn index_exists(manager: &SchemaManager<'_>, index_name: &str) -> Result<bool, DbErr> {
    Ok(manager
        .get_connection()
        .query_one(Statement::from_string(
            manager.get_database_backend(),
            format!("SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = '{index_name}'"),
        ))
        .await?
        .is_some())
}

async fn column_exists(
    manager: &SchemaManager<'_>,
    table_name: &str,
    column_name: &str,
) -> Result<bool, DbErr> {
    Ok(manager
        .get_connection()
        .query_one(Statement::from_string(
            manager.get_database_backend(),
            format!("SELECT 1 FROM pragma_table_info('{table_name}') WHERE name = '{column_name}'"),
        ))
        .await?
        .is_some())
}

async fn table_exists(manager: &SchemaManager<'_>, table_name: &str) -> Result<bool, DbErr> {
    Ok(manager
        .get_connection()
        .query_one(Statement::from_string(
            manager.get_database_backend(),
            format!("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '{table_name}'"),
        ))
        .await?
        .is_some())
}
