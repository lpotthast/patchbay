mod context;
mod git_guard;

use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use context::{ContextOverrides, ResolvedContext, resolve_context};
use git_guard::run_git;
use patchbay_types::{
    AddCommentRequest, AgentReasoningEffort, AgentRunOutputPiece, AgentRunTokenUsageView,
    AgentRunView, AuthorType, ClaimWorkItemRequest, CreateWorkItemLabelRequest,
    CreateWorkItemRequest, FinishWorkItemRequest, ProgressWorkItemRequest, ReleaseWorkItemRequest,
    UpdateProjectMemoryRequest, UpdateWorkItemLabelRequest, UpdateWorkItemRequest, WorkItemView,
};
use rootcause::{Result, prelude::*};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "patchbay")]
#[command(about = "Patchbay agent-facing API relay")]
struct Cli {
    /// Override the Patchbay API URL.
    #[arg(long)]
    api_url: Option<String>,

    /// Override the project context.
    #[arg(long)]
    project: Option<String>,

    /// Override the agent id.
    #[arg(long)]
    agent: Option<String>,

    #[command(subcommand)]
    command: Command,
}

impl Cli {
    fn context_overrides(&self) -> ContextOverrides {
        ContextOverrides {
            api_url: self.api_url.clone(),
            project: self.project.clone(),
            agent_id: self.agent.clone(),
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Work with project-scoped items.
    Item {
        #[command(subcommand)]
        command: ItemCommand,
    },
    /// Read and add item comments.
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
    },
    /// Manage work item labels.
    Label {
        #[command(subcommand)]
        command: LabelCommand,
    },
    /// Read and update project memory.
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Inspect automation runs and logs.
    Automation {
        #[command(subcommand)]
        command: AutomationCommand,
    },
    /// Guarded git entrypoint used by Patchbay automation.
    #[command(hide = true)]
    Git(GitArgs),
}

#[derive(Debug, Subcommand)]
enum ItemCommand {
    /// List project work items.
    List(ItemListArgs),
    /// Show one item; defaults to the claimed item.
    Show(ItemIdArgs),
    /// Create a new work item.
    Create(ItemCreateArgs),
    /// Edit item fields.
    Update(ItemUpdateArgs),
    /// Claim the next available item for this agent.
    Claim(ItemClaimArgs),
    /// Add an agent progress comment.
    Progress(ItemProgressArgs),
    /// Mark an item done with a final report.
    Finish(ItemFinishArgs),
    /// Release an item back to the queue.
    Release(ItemReleaseArgs),
    /// Poll an item and print version changes.
    Watch(ItemWatchArgs),
}

#[derive(Debug, Subcommand)]
enum CommentCommand {
    /// Add a comment to an item.
    Add(CommentAddArgs),
    /// List comments on an item.
    List(ItemIdArgs),
}

#[derive(Debug, Subcommand)]
enum LabelCommand {
    /// List labels on an item.
    List(LabelListArgs),
    /// Add a label to an item.
    Add(LabelAddArgs),
    /// Update a label on an item.
    Update(LabelUpdateArgs),
    /// Delete a label from an item.
    Delete(LabelDeleteArgs),
    /// List labels already used in this project.
    Suggestions(JsonArgs),
}

#[derive(Debug, Subcommand)]
enum MemoryCommand {
    /// Show current project memory.
    Show(JsonArgs),
    /// List project memory change events.
    History(JsonArgs),
    /// Replace project memory.
    Set(MemoryWriteArgs),
    /// Append text to project memory.
    Append(MemoryWriteArgs),
}

#[derive(Debug, Subcommand)]
enum AutomationCommand {
    /// List automation runs.
    Runs(AutomationRunsArgs),
    /// Show one automation run log.
    Log(AutomationRunLogArgs),
}

#[derive(Debug, Args)]
struct GitArgs {
    /// Git arguments passed by the run-specific Patchbay git shim.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Debug, Args)]
struct ItemListArgs {
    /// Filter items by state label value.
    #[arg(long)]
    state: Option<String>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemIdArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemCreateArgs {
    /// Title for the new item.
    #[arg(long)]
    title: String,

    /// Full task description.
    #[arg(long)]
    description: String,

    /// Initial item state label; defaults to open.
    #[arg(long)]
    state: Option<String>,

    /// Agent model override for this item.
    #[arg(long)]
    agent_model: Option<String>,

    /// Reasoning effort override for this item.
    #[arg(long)]
    agent_reasoning_effort: Option<AgentReasoningEffort>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemUpdateArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Replace the item title.
    #[arg(long)]
    title: Option<String>,

    /// Replace the item description.
    #[arg(long)]
    description: Option<String>,

    /// Move the item to a new state label.
    #[arg(long)]
    state: Option<String>,

    /// Set the item-specific agent model.
    #[arg(long)]
    agent_model: Option<String>,

    /// Clear the item-specific agent model.
    #[arg(long)]
    clear_agent_model: bool,

    /// Set the item-specific reasoning effort.
    #[arg(long)]
    agent_reasoning_effort: Option<AgentReasoningEffort>,

    /// Clear the item-specific reasoning effort.
    #[arg(long)]
    clear_agent_reasoning_effort: bool,

    /// Require the current item version.
    #[arg(long)]
    expect_version: Option<i64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemClaimArgs {
    /// State label to claim from.
    #[arg(long, default_value = "open")]
    state: String,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemProgressArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Progress text to record.
    #[arg(long)]
    body: String,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemFinishArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Final report text.
    #[arg(long)]
    report: String,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemReleaseArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Optional release note.
    #[arg(long)]
    comment: Option<String>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ItemWatchArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Only print versions newer than this value.
    #[arg(long)]
    since_version: Option<i64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct LabelListArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct LabelAddArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Label key.
    #[arg(long)]
    key: String,

    /// Optional label value.
    #[arg(long)]
    value: Option<String>,

    /// Require the current item version.
    #[arg(long)]
    expect_version: Option<i64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct LabelUpdateArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Label id to update.
    label_id: i64,

    /// Replacement label key.
    #[arg(long)]
    key: Option<String>,

    /// Replacement label value.
    #[arg(long)]
    value: Option<String>,

    /// Clear the label value.
    #[arg(long)]
    clear_value: bool,

    /// Require the current item version.
    #[arg(long)]
    expect_version: Option<i64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct LabelDeleteArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Label id to delete.
    label_id: i64,

    /// Require the current item version.
    #[arg(long)]
    expect_version: Option<i64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CommentAddArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Comment text.
    #[arg(long)]
    body: String,

    /// Display name for the author.
    #[arg(long)]
    author: Option<String>,

    /// Author type for the comment.
    #[arg(long, default_value = "user")]
    author_type: AuthorType,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationRunsArgs {
    /// Maximum number of runs to show.
    #[arg(long)]
    limit: Option<u64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutomationRunLogArgs {
    /// Automation run id.
    run_id: i64,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct JsonArgs {
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct MemoryWriteArgs {
    /// Memory text to write.
    #[arg(long)]
    body: String,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let context = resolve_context(&cli.context_overrides(), |key| std::env::var(key))
        .context("failed to resolve Patchbay context")?;
    run(cli.command, context).await
}

async fn run(command: Command, context: ResolvedContext) -> Result<()> {
    match command {
        Command::Item { command } => run_item(command, context).await,
        Command::Comment { command } => run_comment(command, context).await,
        Command::Label { command } => run_label(command, context).await,
        Command::Memory { command } => run_memory(command, context).await,
        Command::Automation { command } => run_automation(command, context).await,
        Command::Git(args) => run_git(args.args),
    }
}

async fn run_item(command: ItemCommand, context: ResolvedContext) -> Result<()> {
    let client = context.client();
    let project = context.project()?;
    match command {
        ItemCommand::List(args) => {
            let items = client.list_items(project, args.state.as_deref()).await?;
            output(args.json, &items, || {
                for item in &items {
                    println!(
                        "#{}\t{}\tv{}\t{}",
                        item.id,
                        item_state_label(item),
                        item.version,
                        item.title
                    );
                }
            })
        }
        ItemCommand::Show(args) => {
            let item = client
                .get_item(project, context.item_id(args.item_id)?)
                .await?;
            output(args.json, &item, || {
                println!(
                    "#{} [{}] v{}",
                    item.id,
                    item_state_label(&item),
                    item.version
                );
                println!("{}", item.title);
                if let Some(agent) = &item.claimed_by {
                    println!("claimed by: {agent}");
                }
                if !item.labels.is_empty() {
                    println!(
                        "labels: {}",
                        item.labels
                            .iter()
                            .map(|label| format_label(&label.key, label.value.as_deref()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                println!();
                println!("{}", item.description);
            })
        }
        ItemCommand::Create(args) => {
            let item = client
                .create_item(
                    project,
                    &CreateWorkItemRequest {
                        title: args.title,
                        description: args.description,
                        state: args.state,
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
            let item_id = context.item_id(args.item_id)?;
            let request = UpdateWorkItemRequest {
                title: args.title,
                description: args.description,
                state: args.state,
                agent_model_override: optional_override(args.agent_model, args.clear_agent_model),
                agent_reasoning_effort_override: optional_override(
                    args.agent_reasoning_effort,
                    args.clear_agent_reasoning_effort,
                ),
                expect_version: args.expect_version,
            };
            let item = client.update_item(project, item_id, &request).await?;
            output(args.json, &item, || {
                println!("Updated item #{} v{}", item.id, item.version);
            })
        }
        ItemCommand::Claim(args) => {
            let agent_id = context.agent_id()?;
            let claimed = client
                .claim_item(
                    project,
                    &ClaimWorkItemRequest {
                        agent_id: agent_id.to_owned(),
                        state: args.state.clone(),
                    },
                )
                .await?;
            if let Some(item) = claimed.item {
                output(args.json, &item, || {
                    println!("Claimed item #{} for {}", item.id, agent_id);
                })
            } else if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "claimed": false,
                        "project": project,
                        "state": args.state,
                    }))?
                );
                Ok(())
            } else {
                println!("No matching item available");
                Ok(())
            }
        }
        ItemCommand::Progress(args) => {
            let item_id = context.item_id(args.item_id)?;
            let agent_id = context.agent_id()?;
            let comment = client
                .progress_item(
                    project,
                    item_id,
                    &ProgressWorkItemRequest {
                        agent_id: agent_id.to_owned(),
                        body: args.body,
                    },
                )
                .await?;
            output(args.json, &comment, || {
                println!("Recorded progress comment #{}", comment.id);
            })
        }
        ItemCommand::Finish(args) => {
            let item_id = context.item_id(args.item_id)?;
            let agent_id = context.agent_id()?;
            let item = client
                .finish_item(
                    project,
                    item_id,
                    &FinishWorkItemRequest {
                        agent_id: agent_id.to_owned(),
                        report: args.report,
                    },
                )
                .await?;
            output(args.json, &item, || {
                println!("Finished item #{} v{}", item.id, item.version);
            })
        }
        ItemCommand::Release(args) => {
            let item_id = context.item_id(args.item_id)?;
            let agent_id = context.agent_id()?;
            let item = client
                .release_item(
                    project,
                    item_id,
                    &ReleaseWorkItemRequest {
                        agent_id: agent_id.to_owned(),
                        comment: args.comment,
                    },
                )
                .await?;
            output(args.json, &item, || {
                println!(
                    "Released item #{} back to {}",
                    item.id,
                    item_state_label(&item)
                );
            })
        }
        ItemCommand::Watch(args) => {
            let item_id = context.item_id(args.item_id)?;
            let mut last_version = args.since_version.unwrap_or(0);
            loop {
                let item = client.get_item(project, item_id).await?;
                if item.version > last_version {
                    last_version = item.version;
                    output(args.json, &item, || {
                        println!(
                            "#{}\t{}\tv{}\t{}",
                            item.id,
                            item_state_label(&item),
                            item.version,
                            item.title
                        );
                    })?;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn run_label(command: LabelCommand, context: ResolvedContext) -> Result<()> {
    let client = context.client();
    let project = context.project()?;
    match command {
        LabelCommand::List(args) => {
            let item_id = context.item_id(args.item_id)?;
            let labels = client.list_item_labels(project, item_id).await?;
            output(args.json, &labels, || {
                for label in &labels {
                    println!(
                        "#{}\t{}",
                        label.id,
                        format_label(&label.key, label.value.as_deref())
                    );
                }
            })
        }
        LabelCommand::Add(args) => {
            let item_id = context.item_id(args.item_id)?;
            let item = client
                .add_item_label(
                    project,
                    item_id,
                    &CreateWorkItemLabelRequest {
                        key: args.key,
                        value: args.value,
                    },
                    args.expect_version,
                )
                .await?;
            output(args.json, &item, || {
                println!("Added label on item #{} v{}", item.id, item.version);
            })
        }
        LabelCommand::Update(args) => {
            let item_id = context.item_id(args.item_id)?;
            let request = UpdateWorkItemLabelRequest {
                key: args.key,
                value: optional_override(args.value, args.clear_value),
                expect_version: args.expect_version,
            };
            let item = client
                .update_item_label(project, item_id, args.label_id, &request)
                .await?;
            output(args.json, &item, || {
                println!(
                    "Updated label #{} on item #{} v{}",
                    args.label_id, item.id, item.version
                );
            })
        }
        LabelCommand::Delete(args) => {
            let item_id = context.item_id(args.item_id)?;
            let deleted = client
                .delete_item_label(project, item_id, args.label_id, args.expect_version)
                .await?;
            output(args.json, &deleted, || {
                println!("Deleted label #{}", deleted.label_id);
            })
        }
        LabelCommand::Suggestions(args) => {
            let labels = client.list_project_labels(project).await?;
            output(args.json, &labels, || {
                for label in &labels {
                    println!(
                        "{}\t{}",
                        format_label(&label.key, label.value.as_deref()),
                        label.usage_count
                    );
                }
            })
        }
    }
}

async fn run_comment(command: CommentCommand, context: ResolvedContext) -> Result<()> {
    let client = context.client();
    let project = context.project()?;
    match command {
        CommentCommand::Add(args) => {
            let item_id = context.item_id(args.item_id)?;
            let comment = client
                .add_comment(
                    project,
                    item_id,
                    &AddCommentRequest {
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
            let comments = client
                .list_comments(project, context.item_id(args.item_id)?)
                .await?;
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

async fn run_memory(command: MemoryCommand, context: ResolvedContext) -> Result<()> {
    let client = context.client();
    let project = context.project()?;
    match command {
        MemoryCommand::Show(args) => {
            let memory = client.get_project_memory(project).await?;
            output(args.json, &memory, || {
                println!("{}", memory.memory);
            })
        }
        MemoryCommand::History(args) => {
            let events = client.list_project_memory_events(project).await?;
            output(args.json, &events, || {
                for event in &events {
                    println!(
                        "#{}\t{}\t{}\t{}",
                        event.id,
                        event.operation,
                        event.created_at,
                        event
                            .actor_id
                            .as_deref()
                            .or(event.actor_type.as_deref())
                            .unwrap_or("")
                    );
                }
            })
        }
        MemoryCommand::Set(args) => {
            let agent_id = context.agent_id()?;
            let update = client
                .set_project_memory(
                    project,
                    &UpdateProjectMemoryRequest {
                        agent_id: agent_id.to_owned(),
                        agent_run_id: None,
                        body: args.body,
                    },
                )
                .await?;
            output(args.json, &update, || {
                println!(
                    "Updated memory for project {} with event #{}",
                    update.project.name, update.event.id
                );
            })
        }
        MemoryCommand::Append(args) => {
            let agent_id = context.agent_id()?;
            let update = client
                .append_project_memory(
                    project,
                    &UpdateProjectMemoryRequest {
                        agent_id: agent_id.to_owned(),
                        agent_run_id: None,
                        body: args.body,
                    },
                )
                .await?;
            output(args.json, &update, || {
                println!(
                    "Appended memory for project {} with event #{}",
                    update.project.name, update.event.id
                );
            })
        }
    }
}

async fn run_automation(command: AutomationCommand, context: ResolvedContext) -> Result<()> {
    let client = context.client();
    let project = context.project()?;
    match command {
        AutomationCommand::Runs(args) => {
            let runs = client.list_runs(project, args.limit).await?;
            output(args.json, &runs, || {
                for run in &runs {
                    println!(
                        "#{}\t{}\t{}\t{}\t{}",
                        run.id,
                        run.status,
                        run.tool_name,
                        run_token_usage_text(run),
                        run.result_summary
                    );
                }
            })
        }
        AutomationCommand::Log(args) => {
            let log = client.read_run_log(project, args.run_id).await?;
            output(args.json, &log, || {
                println!("run #{} {}", log.run.id, log.run.status);
                println!("summary: {}", log.run.result_summary);
                println!("tokens: {}", run_token_usage_text(&log.run));
                println!("commit: {}", run_commit_outcome_text(&log.run));
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
    }
}

fn optional_override<T>(value: Option<T>, clear: bool) -> Option<Option<T>> {
    if clear { Some(None) } else { value.map(Some) }
}

fn item_state_label(item: &WorkItemView) -> &str {
    item.state.as_deref().unwrap_or("(no state)")
}

fn format_label(key: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("{key}={value}"),
        None => key.to_owned(),
    }
}

fn run_commit_outcome_text(run: &AgentRunView) -> String {
    let requirement = if run.commit_required {
        "required"
    } else {
        "not required"
    };
    let base = match run.commit_outcome {
        patchbay_types::AgentCommitOutcome::NotEvaluated => "not evaluated".to_owned(),
        patchbay_types::AgentCommitOutcome::NotRequired => "not required by policy".to_owned(),
        patchbay_types::AgentCommitOutcome::Committed => {
            if run.commit_shas.is_empty() {
                "committed".to_owned()
            } else {
                format!("committed {}", run.commit_shas.join(", "))
            }
        }
        patchbay_types::AgentCommitOutcome::SkippedNoChanges => "skipped: no changes".to_owned(),
        patchbay_types::AgentCommitOutcome::SkippedNoGitRepo => {
            "skipped: no git repository".to_owned()
        }
        patchbay_types::AgentCommitOutcome::MissingRequired => "missing required commit".to_owned(),
        patchbay_types::AgentCommitOutcome::Unknown => "unknown".to_owned(),
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

fn output<T>(json: bool, value: &T, text: impl FnOnce()) -> Result<()>
where
    T: Serialize,
{
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        text();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help_output(args: &[&str]) -> String {
        let error = Cli::try_parse_from(args).expect_err("help should stop parsing");
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
        error.to_string()
    }

    fn assert_command_tree_has_help(command: &clap::Command, path: &mut Vec<String>) {
        for arg in command.get_arguments() {
            let id = arg.get_id().as_str();
            if id == "help" || id == "version" {
                continue;
            }
            assert!(
                arg.get_help()
                    .is_some_and(|help| !help.to_string().trim().is_empty()),
                "missing help for argument {id} on {}",
                path.join(" ")
            );
        }

        for subcommand in command.get_subcommands() {
            if subcommand.get_name() == "help" {
                continue;
            }
            path.push(subcommand.get_name().to_owned());
            assert!(
                subcommand
                    .get_about()
                    .or_else(|| subcommand.get_long_about())
                    .is_some_and(|about| !about.to_string().trim().is_empty()),
                "missing help for command {}",
                path.join(" ")
            );
            assert_command_tree_has_help(subcommand, path);
            path.pop();
        }
    }

    #[test]
    fn clap_metadata_covers_every_command_and_argument() {
        let command = <Cli as clap::CommandFactory>::command();
        let mut path = vec![command.get_name().to_owned()];
        assert_command_tree_has_help(&command, &mut path);
    }

    #[test]
    fn help_describes_command_groups_and_subcommands() {
        let root = help_output(&["patchbay", "--help"]);
        assert!(root.contains("Work with project-scoped items"));
        assert!(root.contains("Read and add item comments"));
        assert!(root.contains("Manage work item labels"));
        assert!(root.contains("Read and update project memory"));
        assert!(root.contains("Inspect automation runs and logs"));
        assert!(root.contains("Override the Patchbay API URL"));
        assert!(root.contains("Override the project context"));
        assert!(root.contains("Override the agent id"));

        let item = help_output(&["patchbay", "item", "--help"]);
        assert!(item.contains("List project work items"));
        assert!(item.contains("Show one item; defaults to the claimed item"));
        assert!(item.contains("Create a new work item"));
        assert!(item.contains("Edit item fields"));
        assert!(item.contains("Claim the next available item for this agent"));
        assert!(item.contains("Add an agent progress comment"));
        assert!(item.contains("Mark an item done with a final report"));
        assert!(item.contains("Release an item back to the queue"));
        assert!(item.contains("Poll an item and print version changes"));

        let comment = help_output(&["patchbay", "comment", "--help"]);
        assert!(comment.contains("Add a comment to an item"));
        assert!(comment.contains("List comments on an item"));

        let label = help_output(&["patchbay", "label", "--help"]);
        assert!(label.contains("List labels on an item"));
        assert!(label.contains("Add a label to an item"));
        assert!(label.contains("Update a label on an item"));
        assert!(label.contains("Delete a label from an item"));
        assert!(label.contains("List labels already used in this project"));

        let memory = help_output(&["patchbay", "memory", "--help"]);
        assert!(memory.contains("Show current project memory"));
        assert!(memory.contains("List project memory change events"));
        assert!(memory.contains("Replace project memory"));
        assert!(memory.contains("Append text to project memory"));

        let automation = help_output(&["patchbay", "automation", "--help"]);
        assert!(automation.contains("List automation runs"));
        assert!(automation.contains("Show one automation run log"));
    }

    #[test]
    fn leaf_help_describes_arguments() {
        let create = help_output(&["patchbay", "item", "create", "--help"]);
        assert!(create.contains("Title for the new item"));
        assert!(create.contains("Full task description"));
        assert!(create.contains("Initial item state label"));
        assert!(create.contains("Reasoning effort override for this item"));
        assert!(create.contains("Print JSON instead of text"));

        let update = help_output(&["patchbay", "item", "update", "--help"]);
        assert!(update.contains("Item id; defaults to the claimed item"));
        assert!(update.contains("Move the item to a new state label"));
        assert!(update.contains("Clear the item-specific agent model"));
        assert!(update.contains("Require the current item version"));

        let label_add = help_output(&["patchbay", "label", "add", "--help"]);
        assert!(label_add.contains("Label key"));
        assert!(label_add.contains("Optional label value"));

        let progress = help_output(&["patchbay", "item", "progress", "--help"]);
        assert!(progress.contains("Progress text to record"));

        let comment = help_output(&["patchbay", "comment", "add", "--help"]);
        assert!(comment.contains("Comment text"));
        assert!(comment.contains("Author type for the comment"));

        let memory = help_output(&["patchbay", "memory", "append", "--help"]);
        assert!(memory.contains("Memory text to write"));

        let automation = help_output(&["patchbay", "automation", "log", "--help"]);
        assert!(automation.contains("Automation run id"));
    }
}
