# API Design

Patchbay exposes a custom JSON API for domain workflows and a separate CrudKit API for ordinary admin resources. The standalone CLI uses the custom JSON API through `patchbay-api-client`.

## API Principles

- All workflow operations are project-scoped.
- The server enforces ownership, item state, version safety, and automation rules.
- Custom workflow operations are not CrudKit endpoints.
- Request and response DTOs are shared through `patchbay-types`.
- API clients do not know about database paths or storage internals.

## Custom JSON Endpoints

Project endpoints:

```text
GET  /api/projects/{project}
GET  /api/projects/{project}/settings
GET  /api/projects/{project}/memory
PUT  /api/projects/{project}/memory
POST /api/projects/{project}/memory/append
GET  /api/projects/{project}/memory/events
POST /api/projects/{project}/memory/events/compact
```

Work item endpoints:

```text
GET   /api/projects/{project}/items
POST  /api/projects/{project}/items
GET   /api/projects/{project}/items/{item_id}
PATCH /api/projects/{project}/items/{item_id}
```

Workflow endpoints:

```text
POST /api/projects/{project}/items/claim
POST /api/projects/{project}/items/{item_id}/progress
POST /api/projects/{project}/items/{item_id}/finish
POST /api/projects/{project}/items/{item_id}/release
```

Comment endpoints:

```text
GET  /api/projects/{project}/items/{item_id}/comments
POST /api/projects/{project}/items/{item_id}/comments
```

Automation endpoints:

```text
GET /api/projects/{project}/automation/runs
GET /api/projects/{project}/automation/runs/{run_id}/log
GET /api/projects/{project}/automation/sessions
```

Event endpoints:

```text
GET /api/projects/{project}/events
GET /api/projects/{project}/items/{item_id}/events
```

## Workflow Semantics

`claim` chooses an eligible item in the requested state and assigns it to the requesting agent. It does not use `PATCHBAY_CLAIMED_ITEM_ID` as an implicit input.

`progress` appends an agent progress comment and records a workflow event. The caller must be the claimant unless server policy explicitly allows the update.

`finish` appends a completion report, marks the item done, clears active claim ownership, records finish metadata, and emits events.

`release` appends an optional release comment, clears the claim, returns the item to an available state, and emits events.

`PATCH /items/{item_id}` is for item field updates and supports version safety. It is separate from workflow transitions.

Project memory writes require Patchbay agent attribution in the request body. `PUT /memory` rewrites the complete memory field; `POST /memory/append` appends to the existing memory. Both create `MemoryChanged` events containing the full post-write memory snapshot. Compaction deletes memory history events only; the current project memory remains on the project record.

## CrudKit Endpoints

CrudKit-generated routes are mounted under `/api` for ordinary admin resources:

- projects;
- work items;
- comments;
- agent tools;
- agent runs;
- automation triggers.

CrudKit is not used for custom workflow authority. Admin CRUD can inspect and maintain records, but workflow transitions should use the custom endpoints so server services apply Patchbay rules consistently.

## UI Form Endpoints

The Leptos UI uses server form handlers for operator actions such as:

- creating, updating, and deleting projects;
- updating project prompts, memory, and settings;
- creating, updating, moving, deleting, and commenting on work items;
- starting, stopping, and recovering automation;
- cleaning up worktrees;
- creating, updating, and deleting triggers;
- discovering agent tools;
- picking folders on the local system.

These endpoints are UI integration points, not the stable agent-facing API.

## Errors

API errors should be explicit enough for the CLI to show actionable output. Important error classes include:

- missing project context;
- unknown project or item;
- invalid state transition;
- item already claimed;
- caller does not own the claim;
- stale expected version;
- automation tool unavailable;
- run log unavailable.

The server should prefer structured error responses over plain text so CLI and future clients can distinguish user errors from server failures.
