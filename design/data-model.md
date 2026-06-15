# Data Model

Patchbay stores project-scoped work coordination data. The database schema lives in `patchbay-server`; shared API shapes live in `patchbay-types`.

## Projects

A project is the root scope for work items, automation settings, triggers, comments, runs, and events.

Project data includes:

- stable name and display name;
- filesystem path and path health metadata;
- project system prompt and memory text;
- workspace mode;
- automation concurrency settings;
- stale-claim timeout;
- pull request and worktree cleanup preferences;
- default agent tool, model, and reasoning effort.

All item and automation API calls are project-scoped. Missing project context is an error for agent-facing operations.

Project memory is the project-owned shared memory for agents. Every project memory write creates a project-level `MemoryChanged` event with the full memory snapshot after the write. Memory events carry optional actor and agent-run attribution so a memory change can be traced back to the Patchbay agent/session that wrote it.

## Work Items

Work items are the primary coordination unit.

Core fields include:

- title and description;
- state: `open`, `in_progress`, or `done`;
- monotonically increasing version;
- current claimant and claim timestamps;
- claim expiration timestamp;
- finish timestamp;
- automation eligibility;
- optional agent model and reasoning effort overrides;
- comment count and timestamps.

The version field supports optimistic safety for updates and workflow transitions. Claim ownership is enforced server-side.

## Comments

Comments are attached to work items and are used for user context, agent progress, completion reports, release notes, and discussion.

Comment authors include user, agent, and system author types. The server records author name, body, work item, and creation time.

## Events

Patchbay records workflow and automation events for live UI updates and auditability. Event streams are project-scoped and can also be filtered to a work item.

Events are used by item watch commands, live board updates, and automation visibility. They are not a substitute for the current state stored on projects, work items, comments, and runs.

Memory history is reconstructable from `MemoryChanged` event snapshots until a user compacts memory history. Compaction removes old memory events but does not change the current `projects.memory` value. Agent runs may keep a memory event id reference; readers must tolerate the referenced event being unavailable after compaction.

## Agent Tools

Agent tools describe launchable coding-agent integrations. The current implementation targets Codex. Tool records support discovery and configuration through the admin UI and server services.

Agents launched by Patchbay receive a prepared environment and a CLI on `PATH`; they do not receive database access.

## Agent Runs

An agent run records an automation process.

Run data includes:

- project and optional work item;
- automation mode;
- tool name;
- status: `running`, `completed`, `failed`, or `cancelled`;
- command and working directory;
- worktree path and branch name when applicable;
- process id and exit code;
- log path and prompt path;
- selected agent model and reasoning effort;
- pull request request and URL fields;
- cleanup status;
- timestamps.

Run logs are read through server endpoints. The log file path is an implementation detail and should not be handed to agents as the primary interface.

## Automation Triggers

Automation triggers allow Patchbay to start work from configured events.

Supported trigger kinds include:

- cron schedules;
- work-item-created triggers.

Trigger records include enabled state, mode, tool, prompt, schedule, last and next run metadata, and the last consumed event id when applicable.

## Settings

Project settings control automation behavior:

- workspace mode: current branch, Git branch, or Git worktree;
- maximum concurrent code-edit agents;
- whether refinement agents can run while editing agents are active;
- pull request creation;
- stale-claim timeout;
- worktree cleanup policy;
- default agent tool, model, and reasoning effort.

Settings are applied by server services at launch and workflow boundaries, not by the agent-facing CLI.
