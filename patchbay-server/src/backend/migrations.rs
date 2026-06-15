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
    AutomationClaimable,
    AgentModelOverride,
    AgentReasoningEffortOverride,
    Version,
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
    TriggerKind,
    Schedule,
    Mode,
    ToolName,
    Prompt,
    LastRunAt,
    NextRunAt,
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
                    ColumnDef::new(WorkItems::AutomationClaimable)
                        .boolean()
                        .not_null()
                        .default(true),
                )
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
                    ColumnDef::new(AutomationTriggers::TriggerKind)
                        .string()
                        .not_null(),
                )
                .col(ColumnDef::new(AutomationTriggers::Schedule).string().null())
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
                    ColumnDef::new(AutomationTriggers::LastRunAt)
                        .string()
                        .null(),
                )
                .col(
                    ColumnDef::new(AutomationTriggers::NextRunAt)
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
                .name("idx_automation_triggers_project_kind")
                .table(AutomationTriggers::Table)
                .col(AutomationTriggers::ProjectId)
                .col(AutomationTriggers::TriggerKind)
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
