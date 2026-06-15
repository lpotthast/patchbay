#![recursion_limit = "256"]

mod backend;
mod frontend;
mod shared;

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::{
    backend::{
        agent_tools,
        automation::{self, StartAutomation},
        automation_triggers::{self, CreateAutomationTrigger},
        codex_app_server,
        comments::{self, AddComment},
        items::{self, CreateWorkItem, UpdateWorkItem},
        projects::{self, CreateProject, UpdateProject, UpdateProjectSettings},
        storage::{Store, default_database_path},
        ui,
    },
    shared::view_models::{
        AgentReasoningEffort, AgentRunOutputPiece, AgentToolName, AuthorType, AutomationMode,
        TriggerKind, WorkState, WorkspaceMode, WorktreeCleanupPolicy,
    },
};

#[derive(Debug, Parser)]
#[command(name = "patchbay-server")]
#[command(about = "Trusted Patchbay server and operator commands")]
struct ServerArgs {
    #[arg(long, env = "PATCHBAY_DATABASE")]
    database: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Item {
        #[command(subcommand)]
        command: ItemCommand,
    },
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
    },
    Automation {
        #[command(subcommand)]
        command: AutomationCommand,
    },
    AgentTools {
        #[command(subcommand)]
        command: AgentToolsCommand,
    },
    Serve(ServeArgs),
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    List(JsonArgs),
    Create(ProjectCreateArgs),
    Show(ProjectNameArgs),
    Update(ProjectUpdateArgs),
    Delete(ProjectNameArgs),
    SystemPrompt {
        #[command(subcommand)]
        command: ProjectSystemPromptCommand,
    },
    Memory {
        #[command(subcommand)]
        command: ProjectMemoryCommand,
    },
    Settings {
        #[command(subcommand)]
        command: ProjectSettingsCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ItemCommand {
    List(ItemListArgs),
    Show(ItemIdArgs),
    Create(ItemCreateArgs),
    Update(ItemUpdateArgs),
    Move(ItemMoveArgs),
    Delete(ItemIdArgs),
    Claim(ItemClaimArgs),
    Release(ItemReleaseArgs),
    Progress(ItemProgressArgs),
    Finish(ItemFinishArgs),
    Watch(ItemWatchArgs),
}

#[derive(Debug, Subcommand)]
enum ProjectSystemPromptCommand {
    Show(ProjectNameArgs),
    Set(ProjectTextArgs),
}

#[derive(Debug, Subcommand)]
enum ProjectMemoryCommand {
    Show(ProjectNameArgs),
    Set(ProjectTextArgs),
    Append(ProjectTextArgs),
}

#[derive(Debug, Subcommand)]
enum ProjectSettingsCommand {
    Show(ProjectNameArgs),
    Update(ProjectSettingsUpdateArgs),
}

#[derive(Debug, Subcommand)]
enum CommentCommand {
    Add(CommentAddArgs),
    List(ItemIdArgs),
}

#[derive(Debug, Subcommand)]
enum AutomationCommand {
    Start(AutomationStartArgs),
    Stop(AutomationProjectArgs),
    Status(AutomationProjectArgs),
    Runs(AutomationRunsArgs),
    Log(AutomationRunLogArgs),
    CleanupWorktrees(AutomationCleanupArgs),
    RecoverStaleClaims(AutomationRecoverStaleClaimsArgs),
    Triggers {
        #[command(subcommand)]
        command: AutomationTriggerCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AgentToolsCommand {
    Discover(JsonArgs),
    List(JsonArgs),
    Set(AgentToolSetArgs),
}

#[derive(Debug, Subcommand)]
enum AutomationTriggerCommand {
    List(AutomationProjectArgs),
    Create(AutomationTriggerCreateArgs),
    Delete(AutomationTriggerDeleteArgs),
    RunDue(JsonArgs),
}

#[derive(Debug, Args)]
struct JsonArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ProjectNameArgs {
    #[arg(long)]
    name: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ProjectCreateArgs {
    #[arg(long)]
    name: String,

    #[arg(long)]
    display_name: Option<String>,

    #[arg(long, alias = "repo", value_name = "PATH")]
    path: PathBuf,

    #[arg(long)]
    system_prompt: Option<String>,

    #[arg(long)]
    memory: Option<String>,

    #[arg(long)]
    default_agent_model: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ProjectUpdateArgs {
    #[arg(long)]
    name: String,

    #[arg(long)]
    display_name: Option<String>,

    #[arg(long, alias = "repo", value_name = "PATH")]
    path: Option<PathBuf>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ProjectTextArgs {
    #[arg(long)]
    name: String,

    #[arg(long)]
    body: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ProjectSettingsUpdateArgs {
    #[arg(long)]
    name: String,

    #[arg(long)]
    workspace_mode: Option<WorkspaceMode>,

    #[arg(long)]
    max_code_edit_agents: Option<i64>,

    #[arg(long)]
    allow_refinement_agents_during_editing: Option<bool>,

    #[arg(long)]
    create_pr: Option<bool>,

    #[arg(long)]
    stale_claim_minutes: Option<i64>,

    #[arg(long)]
    worktree_cleanup_policy: Option<WorktreeCleanupPolicy>,

    #[arg(long)]
    default_agent_tool: Option<AgentToolName>,

    #[arg(long)]
    default_agent_model: Option<String>,

    #[arg(long)]
    default_agent_reasoning_effort: Option<AgentReasoningEffort>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ProjectArg {
    #[arg(long)]
    project: String,
}

#[derive(Debug, Args)]
struct ItemListArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long)]
    state: Option<WorkState>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemIdArgs {
    #[command(flatten)]
    project: ProjectArg,

    item_id: i64,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemCreateArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long)]
    title: String,

    #[arg(long)]
    description: String,

    #[arg(long)]
    unclaimable: bool,

    #[arg(long)]
    agent_model: Option<String>,

    #[arg(long)]
    agent_reasoning_effort: Option<AgentReasoningEffort>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemUpdateArgs {
    #[command(flatten)]
    project: ProjectArg,

    item_id: i64,

    #[arg(long)]
    title: Option<String>,

    #[arg(long)]
    description: Option<String>,

    #[arg(long)]
    automation_claimable: Option<bool>,

    #[arg(long)]
    agent_model: Option<String>,

    #[arg(long)]
    clear_agent_model: bool,

    #[arg(long)]
    agent_reasoning_effort: Option<AgentReasoningEffort>,

    #[arg(long)]
    clear_agent_reasoning_effort: bool,

    #[arg(long)]
    expect_version: Option<i64>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemMoveArgs {
    #[command(flatten)]
    project: ProjectArg,

    item_id: i64,

    #[arg(long)]
    state: WorkState,

    #[arg(long)]
    expect_version: Option<i64>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemClaimArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long)]
    agent: Option<String>,

    #[arg(long, default_value = "open")]
    state: WorkState,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemReleaseArgs {
    #[command(flatten)]
    project: ProjectArg,

    item_id: i64,

    #[arg(long)]
    agent: String,

    #[arg(long)]
    comment: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemProgressArgs {
    #[command(flatten)]
    project: ProjectArg,

    item_id: i64,

    #[arg(long)]
    agent: String,

    #[arg(long)]
    body: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemFinishArgs {
    #[command(flatten)]
    project: ProjectArg,

    item_id: i64,

    #[arg(long)]
    agent: String,

    #[arg(long)]
    report: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemWatchArgs {
    #[command(flatten)]
    project: ProjectArg,

    item_id: i64,

    #[arg(long)]
    since_version: Option<i64>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CommentAddArgs {
    #[command(flatten)]
    project: ProjectArg,

    item_id: i64,

    #[arg(long)]
    body: String,

    #[arg(long)]
    author: Option<String>,

    #[arg(long, default_value = "user")]
    author_type: AuthorType,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationStartArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long, default_value = "execute")]
    mode: AutomationMode,

    #[arg(long)]
    tool: Option<AgentToolName>,

    #[arg(long)]
    item_id: Option<i64>,

    #[arg(long)]
    prompt: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationProjectArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationRunsArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long)]
    limit: Option<u64>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationRunLogArgs {
    #[command(flatten)]
    project: ProjectArg,

    run_id: i64,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationCleanupArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long)]
    run_id: Option<i64>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationRecoverStaleClaimsArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long)]
    minutes: Option<i64>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AgentToolSetArgs {
    #[arg(long)]
    tool: AgentToolName,

    #[arg(long)]
    path: PathBuf,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationTriggerCreateArgs {
    #[command(flatten)]
    project: ProjectArg,

    #[arg(long)]
    name: String,

    #[arg(long)]
    kind: TriggerKind,

    #[arg(long)]
    schedule: Option<String>,

    #[arg(long)]
    mode: Option<AutomationMode>,

    #[arg(long)]
    tool: Option<AgentToolName>,

    #[arg(long)]
    disabled: bool,

    #[arg(long, default_value = "")]
    prompt: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationTriggerDeleteArgs {
    #[command(flatten)]
    project: ProjectArg,

    trigger_id: i64,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ServeArgs {
    #[arg(long, default_value_t = default_bind_addr())]
    bind: SocketAddr,
}

fn default_bind_addr() -> SocketAddr {
    std::env::var("LEPTOS_SITE_ADDR")
        .or_else(|_| std::env::var("PATCHBAY_BIND"))
        .unwrap_or_else(|_| "127.0.0.1:4000".to_owned())
        .parse()
        .expect("LEPTOS_SITE_ADDR or PATCHBAY_BIND must be a valid socket address")
}

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}

async fn run() -> Result<()> {
    let args = ServerArgs::parse();
    let database = args.database.unwrap_or_else(default_database_path);
    println!("Database path: {database:?}");
    let store = Store::open(database).await?;

    let command = args.command.unwrap_or_else(|| {
        Command::Serve(ServeArgs {
            bind: default_bind_addr(),
        })
    });

    match command {
        Command::Project { command } => run_project(&store, command).await,
        Command::Item { command } => run_item(&store, command).await,
        Command::Comment { command } => run_comment(&store, command).await,
        Command::Automation { command } => run_automation(&store, command).await,
        Command::AgentTools { command } => run_agent_tools(&store, command).await,
        Command::Serve(args) => {
            println!("Starting Patchbay at http://{}", args.bind);
            ui::serve(store, args.bind).await
        }
    }
}

async fn run_project(store: &Store, command: ProjectCommand) -> Result<()> {
    match command {
        ProjectCommand::List(args) => {
            let projects = projects::list_projects(store).await?;
            output(args.json, &projects, || {
                for project in &projects {
                    println!(
                        "{}\t{}\t{}\t{}",
                        project.name,
                        project.display_name,
                        project.path.as_deref().unwrap_or(""),
                        project_path_status_label(project.path_exists),
                    );
                }
            })
        }
        ProjectCommand::Create(args) => {
            let project = projects::create_project(
                store,
                CreateProject {
                    name: args.name,
                    display_name: args.display_name,
                    path: args.path,
                    default_agent_model: args.default_agent_model,
                    system_prompt: args.system_prompt,
                    memory: args.memory,
                },
            )
            .await?;
            output(args.json, &project, || {
                println!("Created project {} ({})", project.name, project.id);
            })
        }
        ProjectCommand::Show(args) => {
            let project = projects::get_project(store, &args.name).await?;
            output(args.json, &project, || {
                println!("{} ({})", project.display_name, project.name);
                if let Some(path) = &project.path {
                    println!("path: {path}");
                }
                println!(
                    "path status: {}",
                    project_path_status_label(project.path_exists)
                );
                if let Some(checked_at) = &project.path_checked_at {
                    println!("path checked: {checked_at}");
                }
                if !project.system_prompt.is_empty() {
                    println!();
                    println!("system prompt:");
                    println!("{}", project.system_prompt);
                }
                if !project.memory.is_empty() {
                    println!();
                    println!("memory:");
                    println!("{}", project.memory);
                }
            })
        }
        ProjectCommand::Update(args) => {
            let project = projects::update_project(
                store,
                &args.name,
                UpdateProject {
                    display_name: args.display_name,
                    path: args.path,
                },
            )
            .await?;
            output(args.json, &project, || {
                println!("Updated project {} ({})", project.name, project.id);
            })
        }
        ProjectCommand::Delete(args) => {
            projects::delete_project(store, &args.name).await?;
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({ "deleted": true, "name": args.name })
                );
            } else {
                println!("Deleted project {}", args.name);
            }
            Ok(())
        }
        ProjectCommand::SystemPrompt { command } => match command {
            ProjectSystemPromptCommand::Show(args) => {
                let project = projects::get_project(store, &args.name).await?;
                let value = serde_json::json!({
                    "project": project.name,
                    "system_prompt": project.system_prompt,
                });
                output(args.json, &value, || {
                    println!("{}", value["system_prompt"].as_str().unwrap_or(""));
                })
            }
            ProjectSystemPromptCommand::Set(args) => {
                let project = projects::update_system_prompt(store, &args.name, args.body).await?;
                output(args.json, &project, || {
                    println!("Updated system prompt for project {}", project.name);
                })
            }
        },
        ProjectCommand::Memory { command } => match command {
            ProjectMemoryCommand::Show(args) => {
                let project = projects::get_project(store, &args.name).await?;
                let value = serde_json::json!({
                    "project": project.name,
                    "memory": project.memory,
                });
                output(args.json, &value, || {
                    println!("{}", value["memory"].as_str().unwrap_or(""));
                })
            }
            ProjectMemoryCommand::Set(args) => {
                let project = projects::update_memory(store, &args.name, args.body).await?;
                output(args.json, &project, || {
                    println!("Updated memory for project {}", project.name);
                })
            }
            ProjectMemoryCommand::Append(args) => {
                let project = projects::append_memory(store, &args.name, args.body).await?;
                output(args.json, &project, || {
                    println!("Appended memory for project {}", project.name);
                })
            }
        },
        ProjectCommand::Settings { command } => match command {
            ProjectSettingsCommand::Show(args) => {
                let settings = projects::get_settings(store, &args.name).await?;
                output(args.json, &settings, || {
                    println!("workspace_mode: {}", settings.workspace_mode);
                    println!("max_code_edit_agents: {}", settings.max_code_edit_agents);
                    println!(
                        "allow_refinement_agents_during_editing: {}",
                        settings.allow_refinement_agents_during_editing
                    );
                    println!("create_pr: {}", settings.create_pr);
                    println!("stale_claim_minutes: {}", settings.stale_claim_minutes);
                    println!(
                        "worktree_cleanup_policy: {}",
                        settings.worktree_cleanup_policy
                    );
                    println!("default_agent_tool: {}", settings.default_agent_tool);
                    println!(
                        "default_agent_model: {}",
                        settings.default_agent_model.as_deref().unwrap_or("")
                    );
                    println!(
                        "default_agent_reasoning_effort: {}",
                        settings
                            .default_agent_reasoning_effort
                            .map(|effort| effort.to_string())
                            .unwrap_or_default()
                    );
                })
            }
            ProjectSettingsCommand::Update(args) => {
                let settings = projects::update_settings(
                    store,
                    &args.name,
                    UpdateProjectSettings {
                        workspace_mode: args.workspace_mode,
                        max_code_edit_agents: args.max_code_edit_agents,
                        allow_refinement_agents_during_editing: args
                            .allow_refinement_agents_during_editing,
                        create_pr: args.create_pr,
                        stale_claim_minutes: args.stale_claim_minutes,
                        worktree_cleanup_policy: args.worktree_cleanup_policy,
                        default_agent_tool: args.default_agent_tool,
                        default_agent_model: args.default_agent_model.map(Some),
                        default_agent_reasoning_effort: args
                            .default_agent_reasoning_effort
                            .map(Some),
                    },
                )
                .await?;
                output(args.json, &settings, || {
                    println!("Updated settings for project {}", args.name);
                })
            }
        },
    }
}

async fn run_item(store: &Store, command: ItemCommand) -> Result<()> {
    match command {
        ItemCommand::List(args) => {
            let items = items::list_items(store, &args.project.project, args.state).await?;
            output(args.json, &items, || {
                for item in &items {
                    let claim = item
                        .claimed_by
                        .as_deref()
                        .map(|agent| format!("claimed by {agent}"))
                        .unwrap_or_default();
                    println!(
                        "#{}\t{}\tv{}\t{}\t{}",
                        item.id,
                        item.state.label(),
                        item.version,
                        claim,
                        item.title
                    );
                }
            })
        }
        ItemCommand::Show(args) => {
            let item = items::get_item(store, &args.project.project, args.item_id).await?;
            output(args.json, &item, || {
                println!("#{} [{}] v{}", item.id, item.state.label(), item.version);
                println!("{}", item.title);
                if let Some(agent) = &item.claimed_by {
                    println!("claimed by: {agent}");
                }
                if let Some(finished_at) = &item.finished_at {
                    println!("finished at: {finished_at}");
                }
                println!();
                println!("{}", item.description);
            })
        }
        ItemCommand::Create(args) => {
            let item = items::create_item(
                store,
                &args.project.project,
                CreateWorkItem {
                    title: args.title,
                    description: args.description,
                    automation_claimable: !args.unclaimable,
                    agent_model_override: args.agent_model,
                    agent_reasoning_effort_override: args.agent_reasoning_effort,
                },
            )
            .await?;
            output(args.json, &item, || {
                println!("Created item #{}: {}", item.id, item.title);
            })
        }
        ItemCommand::Update(args) => {
            let item = items::update_item(
                store,
                &args.project.project,
                args.item_id,
                UpdateWorkItem {
                    title: args.title,
                    description: args.description,
                    automation_claimable: args.automation_claimable,
                    agent_model_override: optional_override(
                        args.agent_model,
                        args.clear_agent_model,
                    ),
                    agent_reasoning_effort_override: optional_override(
                        args.agent_reasoning_effort,
                        args.clear_agent_reasoning_effort,
                    ),
                    expect_version: args.expect_version,
                },
            )
            .await?;
            output(args.json, &item, || {
                println!("Updated item #{} v{}", item.id, item.version);
            })
        }
        ItemCommand::Move(args) => {
            let item = items::move_item(
                store,
                &args.project.project,
                args.item_id,
                args.state,
                args.expect_version,
            )
            .await?;
            output(args.json, &item, || {
                println!("Moved item #{} to {}", item.id, item.state.label());
            })
        }
        ItemCommand::Delete(args) => {
            items::delete_item(store, &args.project.project, args.item_id).await?;
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({ "deleted": true, "id": args.item_id })
                );
            } else {
                println!("Deleted item #{}", args.item_id);
            }
            Ok(())
        }
        ItemCommand::Claim(args) => {
            let agent_id = args.agent.unwrap_or_else(default_agent_id);
            let claimed =
                items::claim_item(store, &args.project.project, &agent_id, args.state).await?;
            match claimed {
                Some(item) => output(args.json, &item, || {
                    println!("Claimed item #{} for {}", item.id, agent_id);
                }),
                None => {
                    if args.json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "claimed": false,
                                "project": args.project.project,
                                "state": args.state,
                            })
                        );
                    } else {
                        println!("No matching item available");
                    }
                    Ok(())
                }
            }
        }
        ItemCommand::Release(args) => {
            let item = items::release_item(
                store,
                &args.project.project,
                args.item_id,
                &args.agent,
                args.comment,
            )
            .await?;
            output(args.json, &item, || {
                println!("Released item #{} back to {}", item.id, item.state.label());
            })
        }
        ItemCommand::Progress(args) => {
            let comment = items::progress_item(
                store,
                &args.project.project,
                args.item_id,
                &args.agent,
                &args.body,
            )
            .await?;
            output(args.json, &comment, || {
                println!("Recorded progress comment #{}", comment.id);
            })
        }
        ItemCommand::Finish(args) => {
            let item = items::finish_item(
                store,
                &args.project.project,
                args.item_id,
                &args.agent,
                &args.report,
            )
            .await?;
            output(args.json, &item, || {
                println!("Finished item #{} v{}", item.id, item.version);
            })
        }
        ItemCommand::Watch(args) => {
            let mut last_version = args.since_version.unwrap_or(0);
            loop {
                let item = items::get_item(store, &args.project.project, args.item_id).await?;
                if item.version > last_version {
                    last_version = item.version;
                    if args.json {
                        println!("{}", serde_json::to_string(&item)?);
                    } else {
                        println!(
                            "#{}\t{}\tv{}\t{}",
                            item.id,
                            item.state.label(),
                            item.version,
                            item.title
                        );
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn run_comment(store: &Store, command: CommentCommand) -> Result<()> {
    match command {
        CommentCommand::Add(args) => {
            let comment = comments::add_comment(
                store,
                &args.project.project,
                args.item_id,
                AddComment {
                    author_type: args.author_type,
                    author_name: args.author,
                    body: args.body,
                },
            )
            .await?;
            output(args.json, &comment, || {
                println!(
                    "Added comment #{} to item #{}",
                    comment.id, comment.work_item_id
                );
            })
        }
        CommentCommand::List(args) => {
            let comments =
                comments::list_comments(store, &args.project.project, args.item_id).await?;
            output(args.json, &comments, || {
                for comment in &comments {
                    println!(
                        "#{}\t{}\t{}\t{}",
                        comment.id,
                        comment.author_type,
                        comment.author_name.as_deref().unwrap_or(""),
                        comment.body
                    );
                }
            })
        }
    }
}

async fn run_automation(store: &Store, command: AutomationCommand) -> Result<()> {
    match command {
        AutomationCommand::Start(args) => {
            let run = automation::start_automation(
                store,
                &args.project.project,
                StartAutomation {
                    mode: args.mode,
                    tool: args.tool,
                    work_item_id: args.item_id,
                    extra_prompt: args.prompt,
                    trigger: None,
                },
            )
            .await?;
            output(args.json, &run, || {
                println!(
                    "Automation run #{} {} ({})",
                    run.id, run.status, run.result_summary
                );
            })
        }
        AutomationCommand::Stop(args) => {
            let runs = automation::stop_automation(store, &args.project.project).await?;
            output(args.json, &runs, || {
                println!("Cancelled {} running run(s)", runs.len());
            })
        }
        AutomationCommand::Status(args) => {
            let status = automation::automation_status(store, &args.project.project).await?;
            output(args.json, &status, || {
                println!("project: {}", status.project);
                println!("workspace: {}", status.settings.workspace_mode);
                println!(
                    "max_code_edit_agents: {}",
                    status.settings.max_code_edit_agents
                );
                println!("default_tool: {}", status.settings.default_agent_tool);
                println!("running: {}", status.running_runs);
                println!("recent runs: {}", status.recent_runs.len());
            })
        }
        AutomationCommand::Runs(args) => {
            let runs = automation::list_runs(store, &args.project.project, args.limit).await?;
            output(args.json, &runs, || {
                for run in &runs {
                    println!(
                        "#{}\t{}\t{}\t{}\t{}",
                        run.id, run.status, run.mode, run.tool_name, run.result_summary
                    );
                }
            })
        }
        AutomationCommand::Log(args) => {
            let log = automation::read_run_log(store, &args.project.project, args.run_id).await?;
            output(args.json, &log, || {
                println!("run #{} {}", log.run.id, log.run.status);
                println!("summary: {}", log.run.result_summary);
                println!();
                println!("output:");
                print_output_pieces(&log.output);
                if let Some(prompt) = &log.prompt {
                    println!();
                    println!("prompt:");
                    println!("{prompt}");
                }
            })
        }
        AutomationCommand::CleanupWorktrees(args) => {
            let runs =
                automation::cleanup_worktrees(store, &args.project.project, args.run_id).await?;
            output(args.json, &runs, || {
                println!("Evaluated {} run(s) for worktree cleanup", runs.len());
                for run in &runs {
                    println!(
                        "#{}\t{}\t{}",
                        run.id,
                        run.cleanup_status,
                        run.worktree_path.as_deref().unwrap_or("")
                    );
                }
            })
        }
        AutomationCommand::RecoverStaleClaims(args) => {
            let recovered = automation::recover_stale_claims_for_project(
                store,
                &args.project.project,
                args.minutes,
            )
            .await?;
            output(args.json, &recovered, || {
                println!("Recovered {} stale claim(s)", recovered.len());
                for claim in &recovered {
                    println!("#{}\t{}", claim.item_id, claim.agent_id);
                }
            })
        }
        AutomationCommand::Triggers { command } => match command {
            AutomationTriggerCommand::List(args) => {
                let triggers =
                    automation_triggers::list_triggers(store, &args.project.project).await?;
                output(args.json, &triggers, || {
                    for trigger in &triggers {
                        println!(
                            "#{}\t{}\t{}\t{}",
                            trigger.id,
                            trigger.name,
                            trigger.trigger_kind,
                            if trigger.enabled {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        );
                    }
                })
            }
            AutomationTriggerCommand::Create(args) => {
                let trigger = automation_triggers::create_trigger(
                    store,
                    &args.project.project,
                    CreateAutomationTrigger {
                        name: args.name,
                        enabled: !args.disabled,
                        trigger_kind: args.kind,
                        schedule: args.schedule,
                        mode: args.mode,
                        tool_name: args.tool,
                        prompt: args.prompt,
                    },
                )
                .await?;
                output(args.json, &trigger, || {
                    println!(
                        "Created automation trigger #{}: {}",
                        trigger.id, trigger.name
                    );
                })
            }
            AutomationTriggerCommand::Delete(args) => {
                automation_triggers::delete_trigger(store, &args.project.project, args.trigger_id)
                    .await?;
                if args.json {
                    println!(
                        "{}",
                        serde_json::json!({ "deleted": true, "id": args.trigger_id })
                    );
                } else {
                    println!("Deleted automation trigger #{}", args.trigger_id);
                }
                Ok(())
            }
            AutomationTriggerCommand::RunDue(args) => {
                let outcomes = automation_triggers::run_due_triggers(store).await?;
                output(args.json, &outcomes, || {
                    println!("Evaluated triggers; {} run attempt(s)", outcomes.len());
                })
            }
        },
    }
}

async fn run_agent_tools(store: &Store, command: AgentToolsCommand) -> Result<()> {
    match command {
        AgentToolsCommand::Discover(args) => {
            let tools = agent_tools::discover_tools(store).await?;
            let status = codex_app_server::app_server_status(store).await;
            output(args.json, &tools, || {
                for tool in &tools {
                    println!(
                        "{}\t{}",
                        tool.tool_name,
                        tool.effective_path.as_deref().unwrap_or("")
                    );
                }
                if !status.usable {
                    println!(
                        "{}",
                        codex_app_server::operator_guidance(&status).join("\n")
                    );
                }
            })
        }
        AgentToolsCommand::List(args) => {
            let tools = agent_tools::list_tools(store).await?;
            output(args.json, &tools, || {
                for tool in &tools {
                    println!(
                        "{}\t{}",
                        tool.tool_name,
                        tool.effective_path.as_deref().unwrap_or("")
                    );
                }
            })
        }
        AgentToolsCommand::Set(args) => {
            let tool = agent_tools::set_tool_path(store, args.tool, args.path).await?;
            output(args.json, &tool, || {
                println!(
                    "Set {} to {}",
                    tool.tool_name,
                    tool.effective_path.as_deref().unwrap_or("")
                );
            })
        }
    }
}

fn default_agent_id() -> String {
    std::env::var("AGENT_ID").unwrap_or_else(|_| "manual-agent".to_owned())
}

fn project_path_status_label(path_exists: bool) -> &'static str {
    if path_exists { "exists" } else { "missing" }
}

fn optional_override<T>(value: Option<T>, clear: bool) -> Option<Option<T>> {
    if clear { Some(None) } else { value.map(Some) }
}

fn print_output_pieces(output: &[AgentRunOutputPiece]) {
    if output.is_empty() {
        println!("(empty)");
        return;
    }
    for piece in output {
        println!("[#{} {}] {}", piece.sequence, piece.kind, piece.title);
        if !piece.body.trim().is_empty() {
            println!("{}", piece.body);
        }
        if let Some(tool_output) = output_metadata_text(piece) {
            println!("output:");
            println!("{tool_output}");
        }
    }
}

fn output_metadata_text(piece: &AgentRunOutputPiece) -> Option<String> {
    ["output", "result", "content_items", "error"]
        .into_iter()
        .find_map(|key| metadata_value_text(&piece.metadata, key))
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

fn output<T, F>(json: bool, value: &T, human: F) -> Result<()>
where
    T: serde::Serialize,
    F: FnOnce(),
{
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        human();
    }
    Ok(())
}
