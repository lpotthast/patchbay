# Data Model

Patchbay stores project-scoped work coordination data. The database schema lives in `patchbay-server`; shared API shapes live in `patchbay-types`.

## Projects

A project is the root scope for work items, automation settings, automation rules, comments, runs, and events.

Project data includes:

- stable name and display name;
- filesystem path and path health metadata;
- project system prompt and memory text;
- workspace mode;
- automation concurrency settings;
- stale-claim timeout;
- pull request and worktree cleanup preferences;
- default agent tool, model, and reasoning effort.
- agent sandbox mode, extra writable roots, and mutable Git command policy.

All item and automation API calls are project-scoped. Missing project context is an error for agent-facing operations.

The project system prompt is the project-owned instruction text included in automation prompts. Every project system prompt write creates a project-level `SystemPromptChanged` event with the full prompt snapshot after the write. System prompt events carry optional actor and agent-run attribution so a prompt change can be traced back to the Patchbay user or agent/session that wrote it.

Project memory is the project-owned shared memory for agents. Every project memory write creates a project-level `MemoryChanged` event with the full memory snapshot after the write. Memory events carry optional actor and agent-run attribution so a memory change can be traced back to the Patchbay agent/session that wrote it.

## Work Items

Work items are the primary coordination unit.

Core fields include:

- title and description;
- monotonically increasing version;
- current claimant and claim timestamps;
- claim expiration timestamp;
- finish timestamp;
- optional agent model and reasoning effort overrides;
- comment count and timestamps.

Work item labels are project-scoped item metadata. A label has a key and an optional value, such as `bug`, `severity=high`, or `state=open`. Labels can be edited by human operators and agents. The `state` label is Patchbay's built-in workflow hook for claim, finish, release, and default automation transitions.

Patchbay also uses hardcoded workflow labels. `patchbay:claimed-from-state=<state-label>` is transient claim bookkeeping so release can restore the state an item came from. `patchbay:automation-blocked` marks released, non-operable work that automation should skip until the label is removed.

Work item states are project-scoped records with an identifier, display name, and position. They define the authored values that operators should use for the `state` label. New projects start with `idea`, `open`, `in_progress`, and `done` states.

Swim-lanes are project-scoped records with an identifier, display name, position, item order, item creation flag, and a CrudKit `Condition`-shaped filter stored as JSON. Lane filters use work item label keys as `column_name` values, so a lane can show `state=open`, `severity=high`, or nested label combinations. New projects start with lanes that mirror the default states by filtering on `state=<state-identifier>`, but users can add, rename, reorder, remove, or redefine lanes independently from authored states. New projects also get editable work-consuming automations for ordinary open work, needs-refinement routing, and needs-verification routing. The ordinary open-work default targets `state=open` while excluding the refinement and verification routing labels.

The version field supports optimistic safety for updates and workflow transitions. Claim ownership is enforced server-side.

## Comments

Comments are attached to work items and are used for user context, agent progress, completion reports, release notes, and discussion.

Comment authors include user, agent, and system author types. The server records author name, body, work item, and creation time.

## Events

Patchbay records workflow and automation events for live UI updates and auditability. Event streams are project-scoped and can also be filtered to a work item.

Events are used by item watch commands, live board updates, and automation visibility. They are not a substitute for the current state stored on projects, work item labels, comments, and runs.

System prompt history is reconstructable from `SystemPromptChanged` event snapshots until a user compacts system prompt history. Compaction removes old system prompt events but does not change the current `projects.system_prompt` value.

Memory history is reconstructable from `MemoryChanged` event snapshots until a user compacts memory history. Compaction removes old memory events but does not change the current `projects.memory` value. Agent runs may keep a memory event id reference; readers must tolerate the referenced event being unavailable after compaction.

## Agent Tools

Agent tools describe launchable coding-agent integrations. The current implementation targets Codex. Tool records support discovery and configuration through the admin UI and server services.

Agents launched by Patchbay receive a prepared environment and a CLI on `PATH`; they do not receive database access.

## Agent Runs

An agent run records an automation process.

Run data includes:

- project and optional work item;
- tool name;
- status: `running`, `completed`, `failed`, or `cancelled`;
- command and working directory;
- worktree path and branch name when applicable;
- process id and exit code;
- log path and prompt path;
- selected agent model and reasoning effort;
- Codex token usage when reported: input tokens, cached input tokens, and output tokens;
- commit policy outcome: whether a commit was required, the commit outcome status, and created commit SHA(s);
- pull request request and URL fields;
- cleanup status;
- timestamps.

Run logs are read through server endpoints. The log file path is an implementation detail and should not be handed to agents as the primary interface.

## Automation

Automation rules allow Patchbay to evaluate configured activation conditions. Evaluation is cheap. The result is either a new work item or an agent run scheduled against an existing work item.

Automation records have an `activation` and an `effect`.

Supported activations include:

- `manual`: evaluated only when a user queues an evaluation;
- `work_item`: polls for unclaimed work matching the selector on the configured schedule while project automation is running;
- `cron`: evaluates on the configured schedule;
- `work_item_created`: evaluates for newly created work items.

Supported effects are:

- `produce_work`: creates a work item from the automation prompt and does not launch an agent;
- `consume_work`: schedules an agent run for a matching work item.

Automation records include enabled state, activation, effect, tool, prompt, required schedule, priority, evaluation count, queued evaluation count, last and next evaluation metadata, and the last consumed event id when applicable. Work-consuming automation can include a CrudKit `Condition`-shaped work-item selector. Selector clauses use label keys as `column_name` values, so nested `All` and `Any` groups can model rules such as `state=open AND (bug OR severity=high)`. Patchbay implicitly excludes `patchbay:automation-blocked` from automation claims.

Default project automation rules are ordinary editable records. Patchbay creates and migrates these defaults:

- `Claim open work`: consume-work, selector `state=open` plus absence of `needs-refinement` and `needs-verification`.
- `Refine needs-refinement work`: consume-work, selector requiring the `needs-refinement` label.
- `Verify needs-verification work`: consume-work, selector requiring the `needs-verification` label.

The refiner and verifier prompts instruct agents to update item title, description, comments, and labels, remove the triggering label when complete, and leave the underlying implementation work unfinished for later automation or humans.

## Settings

Project settings control automation behavior:

- workspace mode: current branch, Git branch, or Git worktree;
- maximum concurrent code-edit agents;
- whether refinement agents can run while editing agents are active;
- pull request creation;
- auto-commit behavior for current-branch automation;
- commit standard text used in generated agent commit instructions;
- failure revert strategy for current-branch automation: manual revert or Git reset;
- mutable Git command policy: whether agents may use `git add`, `git commit`, `git push`, and `git reset`, plus whether hard reset is never allowed or only allowed in isolated branch/worktree runs;
- stale-claim timeout;
- worktree cleanup policy;
- default agent tool, model, and reasoning effort;
- agent sandbox mode and extra writable roots.

Settings are applied by server services at launch and workflow boundaries, not by the agent-facing CLI.
