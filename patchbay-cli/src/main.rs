mod context;
mod git_guard;
mod render;

use std::{
    io::{self, Write},
    time::Duration,
};

use clap::{Args, Parser, Subcommand};
use context::{ContextOverrides, ResolvedContext, resolve_context};
use git_guard::run_git;
use patchbay_types::{
    AddCommentRequest, AgentReasoningEffort, AuthorType, ClaimWorkItemRequest,
    CreateWorkItemLabelRequest, CreateWorkItemRelationshipRequest, CreateWorkItemRequest,
    FinishWorkItemRequest, ProgressWorkItemRequest, ReleaseWorkItemRequest,
    RequestFeedbackWorkItemRequest, UpdateProjectMemoryRequest, UpdateWorkItemLabelRequest,
    UpdateWorkItemRelationshipRequest, UpdateWorkItemRequest,
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
    /// Manage directed relationships between work items.
    Relationship {
        #[command(subcommand)]
        command: RelationshipCommand,
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
    /// Ask the user for feedback and pause automation.
    RequestFeedback(ItemRequestFeedbackArgs),
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
enum RelationshipCommand {
    /// List relationships touching an item.
    List(RelationshipListArgs),
    /// Create a relationship from an item to a target item.
    Add(RelationshipAddArgs),
    /// Update a relationship kind.
    Update(RelationshipUpdateArgs),
    /// Delete a relationship.
    Delete(RelationshipDeleteArgs),
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
struct ItemRequestFeedbackArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Feedback request to show the user.
    #[arg(long)]
    body: String,

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
struct RelationshipListArgs {
    /// Item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RelationshipAddArgs {
    /// Source item id; defaults to the claimed item when available.
    item_id: Option<i64>,

    /// Target item id.
    #[arg(long)]
    target: i64,

    /// Free-form relationship kind.
    #[arg(long)]
    kind: String,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RelationshipUpdateArgs {
    /// Relationship id to update.
    relationship_id: i64,

    /// Replacement free-form relationship kind.
    #[arg(long)]
    kind: String,

    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RelationshipDeleteArgs {
    /// Relationship id to delete.
    relationship_id: i64,

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
        Command::Relationship { command } => run_relationship(command, context).await,
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
            output(args.json, &items, |output| {
                render::write_item_rows(output, &items)
            })
        }
        ItemCommand::Show(args) => {
            let item = client
                .get_item(project, context.item_id(args.item_id)?)
                .await?;
            output(args.json, &item, |output| {
                render::write_item_detail(output, &item)
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
                        initial_labels: Vec::new(),
                    },
                )
                .await?;
            output(args.json, &item, |output| {
                writeln!(output, "Created item #{}: {}", item.id, item.title)
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
            output(args.json, &item, |output| {
                writeln!(output, "Updated item #{} v{}", item.id, item.version)
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
                output(args.json, &item, |output| {
                    writeln!(output, "Claimed item #{} for {}", item.id, agent_id)
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
            output(args.json, &comment, |output| {
                writeln!(output, "Recorded progress comment #{}", comment.id)
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
            output(args.json, &item, |output| {
                writeln!(output, "Finished item #{} v{}", item.id, item.version)
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
            output(args.json, &item, |output| {
                writeln!(
                    output,
                    "Released item #{} back to {}",
                    item.id,
                    render::item_state_label(&item)
                )
            })
        }
        ItemCommand::RequestFeedback(args) => {
            let item_id = context.item_id(args.item_id)?;
            let agent_id = context.agent_id()?;
            let item = client
                .request_item_feedback(
                    project,
                    item_id,
                    &RequestFeedbackWorkItemRequest {
                        agent_id: agent_id.to_owned(),
                        body: args.body,
                    },
                )
                .await?;
            output(args.json, &item, |output| {
                writeln!(
                    output,
                    "Requested feedback for item #{} and restored state to {}",
                    item.id,
                    render::item_state_label(&item)
                )
            })
        }
        ItemCommand::Watch(args) => {
            let item_id = context.item_id(args.item_id)?;
            let mut last_version = args.since_version.unwrap_or(0);
            loop {
                let item = client.get_item(project, item_id).await?;
                if item.version > last_version {
                    last_version = item.version;
                    output(args.json, &item, |output| {
                        render::write_item_row(output, &item)
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
            output(args.json, &labels, |output| {
                render::write_item_labels(output, &labels)
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
            output(args.json, &item, |output| {
                writeln!(output, "Added label on item #{} v{}", item.id, item.version)
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
            output(args.json, &item, |output| {
                writeln!(
                    output,
                    "Updated label #{} on item #{} v{}",
                    args.label_id, item.id, item.version
                )
            })
        }
        LabelCommand::Delete(args) => {
            let item_id = context.item_id(args.item_id)?;
            let deleted = client
                .delete_item_label(project, item_id, args.label_id, args.expect_version)
                .await?;
            output(args.json, &deleted, |output| {
                writeln!(output, "Deleted label #{}", deleted.label_id)
            })
        }
        LabelCommand::Suggestions(args) => {
            let labels = client.list_project_labels(project).await?;
            output(args.json, &labels, |output| {
                render::write_project_label_suggestions(output, &labels)
            })
        }
    }
}

async fn run_relationship(command: RelationshipCommand, context: ResolvedContext) -> Result<()> {
    let client = context.client();
    let project = context.project()?;
    match command {
        RelationshipCommand::List(args) => {
            let item_id = context.item_id(args.item_id)?;
            let relationships = client.list_item_relationships(project, item_id).await?;
            output(args.json, &relationships, |output| {
                render::write_relationship_rows(output, &relationships)
            })
        }
        RelationshipCommand::Add(args) => {
            let item_id = context.item_id(args.item_id)?;
            let relationship = client
                .create_item_relationship(
                    project,
                    item_id,
                    &CreateWorkItemRelationshipRequest {
                        target_work_item_id: args.target,
                        kind: args.kind,
                    },
                )
                .await?;
            output(args.json, &relationship, |output| {
                writeln!(
                    output,
                    "Created relationship #{}: #{} {} #{}",
                    relationship.relationship.id,
                    relationship.relationship.source_work_item_id,
                    relationship.relationship.kind,
                    relationship.relationship.target_work_item_id
                )
            })
        }
        RelationshipCommand::Update(args) => {
            let relationship = client
                .update_relationship(
                    project,
                    args.relationship_id,
                    &UpdateWorkItemRelationshipRequest { kind: args.kind },
                )
                .await?;
            output(args.json, &relationship, |output| {
                render::write_relationship_view(output, &relationship, "Updated")
            })
        }
        RelationshipCommand::Delete(args) => {
            let deleted = client
                .delete_relationship(project, args.relationship_id)
                .await?;
            output(args.json, &deleted, |output| {
                render::write_relationship_view(output, &deleted.relationship, "Deleted")
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
            output(args.json, &comment, |output| {
                writeln!(
                    output,
                    "Added comment #{} to item #{}",
                    comment.id, comment.work_item_id
                )
            })
        }
        CommentCommand::List(args) => {
            let comments = client
                .list_comments(project, context.item_id(args.item_id)?)
                .await?;
            output(args.json, &comments, |output| {
                render::write_comments(output, &comments)
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
            output(args.json, &memory, |output| {
                writeln!(output, "{}", memory.memory)
            })
        }
        MemoryCommand::History(args) => {
            let events = client.list_project_memory_events(project).await?;
            output(args.json, &events, |output| {
                render::write_memory_events(output, &events)
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
            output(args.json, &update, |output| {
                writeln!(
                    output,
                    "Updated memory for project {} with event #{}",
                    update.project.name, update.event.id
                )
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
            output(args.json, &update, |output| {
                writeln!(
                    output,
                    "Appended memory for project {} with event #{}",
                    update.project.name, update.event.id
                )
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
            output(args.json, &runs, |output| {
                render::write_automation_runs(output, &runs)
            })
        }
        AutomationCommand::Log(args) => {
            let log = client.read_run_log(project, args.run_id).await?;
            output(args.json, &log, |output| {
                render::write_run_log(output, &log)
            })
        }
    }
}

fn optional_override<T>(value: Option<T>, clear: bool) -> Option<Option<T>> {
    if clear { Some(None) } else { value.map(Some) }
}

fn output<T>(
    json: bool,
    value: &T,
    text: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> Result<()>
where
    T: Serialize,
{
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if json {
        serde_json::to_writer_pretty(&mut output, value).context("failed to write JSON output")?;
        writeln!(output).context("failed to write CLI output")?;
    } else {
        text(&mut output).context("failed to write CLI output")?;
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
        assert!(root.contains("Manage directed relationships between work items"));
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
        assert!(item.contains("Ask the user for feedback and pause automation"));
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

        let relationship = help_output(&["patchbay", "relationship", "--help"]);
        assert!(relationship.contains("List relationships touching an item"));
        assert!(relationship.contains("Create a relationship from an item to a target item"));
        assert!(relationship.contains("Update a relationship kind"));
        assert!(relationship.contains("Delete a relationship"));

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

        let relationship_add = help_output(&["patchbay", "relationship", "add", "--help"]);
        assert!(relationship_add.contains("Source item id; defaults to the claimed item"));
        assert!(relationship_add.contains("Target item id"));
        assert!(relationship_add.contains("Free-form relationship kind"));

        let relationship_update = help_output(&["patchbay", "relationship", "update", "--help"]);
        assert!(relationship_update.contains("Relationship id to update"));
        assert!(relationship_update.contains("Replacement free-form relationship kind"));

        let progress = help_output(&["patchbay", "item", "progress", "--help"]);
        assert!(progress.contains("Progress text to record"));

        let request_feedback = help_output(&["patchbay", "item", "request-feedback", "--help"]);
        assert!(request_feedback.contains("Feedback request to show the user"));

        let comment = help_output(&["patchbay", "comment", "add", "--help"]);
        assert!(comment.contains("Comment text"));
        assert!(comment.contains("Author type for the comment"));

        let memory = help_output(&["patchbay", "memory", "append", "--help"]);
        assert!(memory.contains("Memory text to write"));

        let automation = help_output(&["patchbay", "automation", "log", "--help"]);
        assert!(automation.contains("Automation run id"));
    }
}
