# UI Design

Patchbay's web UI is an operator surface for project setup, workflow visibility, automation control, and admin maintenance. It is server-rendered and hydrated with Leptos.

## Routes

Primary UI routes include:

```text
/                                      current workflow surface
/projects                              project administration
/automation                            automation rule administration
/runs                                  automation run visibility
/api/docs                              local API reference
/projects/:project/items/:item_id      item detail
/projects/:project/automation/runs/:run_id/log
/error
```

The UI should keep project context visible and avoid hiding workflow state behind generic admin tables.

## Workflow Surface

The main workflow surface should make these states easy to inspect:

- backlog and all work;
- project-defined swim-lanes based on lane filters and lane ordering;
- in-progress work and claimant, including the triggering automation source when available and a frontend-derived elapsed claim timer from the claim start time;
- recent comments and progress;
- automation status;
- stale or blocked work;
- feedback-requested work that is waiting for a user answer;
- run logs and run outcomes, including linked operated work items, live output for active runs, active-run cancellation, commit outcome, and created commit SHA visibility.
- per-run Codex token usage when reported by the agent runtime.
- Patchbay-owned workflow labels such as `state`, `patchbay:claimed-from-state`, `patchbay:automation-blocked`, and `patchbay:feedback-requested`.

Board and item-detail interactions call server actions or custom API endpoints so workflow rules remain centralized.
Human-authored rich prose fields such as work item descriptions and automation prompts should use the Tiptap-backed editor in create and edit flows, while structured multiline fields such as selectors, writable-root lists, memory history, and commit policy text stay plain text controls.

## Admin Surfaces

CrudKit is appropriate for ordinary resource administration:

- projects;
- work items;
- work item states;
- swim-lanes;
- comments;
- agent tools;
- agent runs;
- automation rules.

Patchbay-specific actions such as claim, release, finish, request feedback, automation launch, stale-claim recovery, and run-log viewing should remain custom UI flows. These actions carry workflow semantics that generic CRUD controls should not duplicate.

Work item state and swim-lane authoring live on project administration surfaces, not the main board. The board shows small lane edit controls that navigate to the selected swim-lane editor. New item state choices come from authored work item states. Lane add controls may preselect a state when a lane filter is state-backed.

The Codex app-server status panel should guide setup failures directly. When
Patchbay's managed Codex home is not signed in, the panel shows the exact
`CODEX_HOME` login command, the managed home path, and a refresh action instead
of relying on users to reconstruct the command from server logs.

## Project Settings

Project settings should expose:

- filesystem path, path health, and Git repository status;
- copy/open actions for the project folder and available RustRover or VS Code editor targets;
- system prompt and memory;
- system prompt and memory history snapshots, with manual history compaction;
- workspace mode;
- agent concurrency for mutating and read-only automation;
- pull request creation;
- current-branch auto-commit behavior;
- commit standard text for generated agent commit messages;
- current-branch failure revert strategy;
- mutable Git command policy as structured controls for `git add`, `git commit`, `git push`, `git reset`, and hard-reset mode;
- stale-claim timeout;
- worktree cleanup policy;
- default agent tool, model, and reasoning effort.

Settings changes should go through server handlers and be reflected in automation launches without requiring agents to know settings internals.
Selector/prompt-based automations do not expose a project-level refinement concurrency exception in settings. Read-only automation concurrency is a general setting, not a refinement-specific bypass.

Codex configuration generated from project settings should not be exposed as raw TOML in the main UI. Operators configure supported policy fields, and Patchbay generates the per-project Codex config and rules.

When a selected project uses the current-branch workspace mode, the top bar should include an Auto-Commit toggle next to the automation Start/Stop control so operators can quickly decide whether completed current-branch work should be committed by the agent.

Quick settings controls such as the top-bar Auto-Commit toggle should update optimistically in the hydrated UI and send the persistence request in the background. If the request fails, the control should roll back to its previous state instead of navigating or reloading the page.

The board and run detail views should make workspaces directly reachable. Project-level actions use the configured project path; run-level actions use the recorded run working directory so Git worktree runs can be opened in the exact folder the agent edited. Editor opening is a server-local fixed allowlist for RustRover and VS Code; unavailable editors should not be shown, and browser requests must not accept arbitrary commands. The board workspace panel should state whether the project path is in a Git repository and, when it is, show the current branch plus added/deleted line counts.

Automation rule administration should show and edit each work-consuming rule's mutability with `mutating` and `read_only` choices. Automation status should show total running runs plus separate mutating and read-only counts, and run list/detail views should display the persisted run mutability so historical logs remain understandable after a rule changes.

## Live Updates

The UI uses project and item event streams to refresh workflow state. Event streams are hints for refreshing the current view; persisted records remain the source of truth.

## Browser Coverage

Browser coverage lives in `patchbay-server/tests/browser_test.rs` and is run explicitly with:

```text
just browser-test
```

The browser test should continue to cover UI placement and workflow visibility after changes to Leptos layouts, generated admin surfaces, or automation controls.
