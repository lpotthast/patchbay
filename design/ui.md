# UI Design

Patchbay's web UI is an operator surface for project setup, workflow visibility, automation control, and admin maintenance. It is server-rendered and hydrated with Leptos.

## Routes

Primary UI routes include:

```text
/                                      current workflow surface
/projects                              project administration
/triggers                              automation trigger administration
/api/docs                              local API reference
/projects/:project/items/:item_id      item detail
/projects/:project/automation/runs/:run_id/log
/error
```

The UI should keep project context visible and avoid hiding workflow state behind generic admin tables.

## Workflow Surface

The main workflow surface should make these states easy to inspect:

- open work;
- in-progress work and claimant;
- done work;
- recent comments and progress;
- automation status;
- stale or blocked work;
- run logs and run outcomes.

Board and item-detail interactions call server actions or custom API endpoints so workflow rules remain centralized.

## Admin Surfaces

CrudKit is appropriate for ordinary resource administration:

- projects;
- work items;
- comments;
- agent tools;
- agent runs;
- automation triggers.

Patchbay-specific actions such as claim, release, finish, automation launch, stale-claim recovery, and run-log viewing should remain custom UI flows. These actions carry workflow semantics that generic CRUD controls should not duplicate.

The Codex app-server status panel should guide setup failures directly. When
Patchbay's managed Codex home is not signed in, the panel shows the exact
`CODEX_HOME` login command, the managed home path, and a refresh action instead
of relying on users to reconstruct the command from server logs.

## Project Settings

Project settings should expose:

- filesystem path and path health;
- copy/open actions for the project folder and IDE;
- system prompt and memory;
- memory history snapshots and manual memory-history compaction;
- workspace mode;
- agent concurrency;
- refinement policy;
- pull request creation;
- stale-claim timeout;
- worktree cleanup policy;
- default agent tool, model, and reasoning effort.

Settings changes should go through server handlers and be reflected in automation launches without requiring agents to know settings internals.

The board and run detail views should make workspaces directly reachable. Project-level actions use the configured project path; run-level actions use the recorded run working directory so Git worktree runs can be opened in the exact folder the agent edited. IDE opening is a server-local action controlled by `PATCHBAY_WORKSPACE_IDE` and must not accept arbitrary commands from browser requests.

## Live Updates

The UI uses project and item event streams to refresh workflow state. Event streams are hints for refreshing the current view; persisted records remain the source of truth.

## Browser Coverage

Browser coverage lives in `patchbay-server/tests/browser_test.rs` and is run explicitly with:

```text
just browser-test
```

The browser test should continue to cover UI placement and workflow visibility after changes to Leptos layouts, generated admin surfaces, or automation controls.
