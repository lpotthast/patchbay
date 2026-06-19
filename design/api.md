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

`POST /items` creates the item, its canonical `state=<value>` label, and any
`initial_labels` supplied in the request in one server-side operation. Initial
labels use the same key/value shape as label-create requests; keys and values
are trimmed, empty values become value-less labels, duplicate keys are rejected,
and `state` must be supplied through the create request's `state` field rather
than duplicated in `initial_labels`. The backwards-compatible `labels` alias is
accepted for `initial_labels`.

Work item relationship endpoints:

```text
GET    /api/projects/{project}/items/{item_id}/relationships
POST   /api/projects/{project}/items/{item_id}/relationships
PATCH  /api/projects/{project}/relationships/{relationship_id}
DELETE /api/projects/{project}/relationships/{relationship_id}
```

Relationship list responses include the relationship id, kind, direction relative to the requested item (`outgoing` when the item is the source, `incoming` when the item is the target), source item summary, target item summary, and timestamps. Create requests use the path item as the source and provide `target_work_item_id` plus a free-form `kind`. Update requests replace only the trimmed kind. Delete responses include the deleted relationship snapshot.

Workflow endpoints:

```text
POST /api/projects/{project}/items/claim
POST /api/projects/{project}/items/{item_id}/progress
POST /api/projects/{project}/items/{item_id}/finish
POST /api/projects/{project}/items/{item_id}/release
POST /api/projects/{project}/items/{item_id}/request-feedback
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

Automation run responses use `AgentRunView`, which includes run mutability (`mutating` or `read_only`) and reported Codex token usage for the run when available. Usage is reported as input tokens, cached input tokens, output tokens, and a derived total. Run-log responses include active in-memory session output while a run is still ongoing and fall back to the persisted output log when no active session is present.

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

`request-feedback` appends an agent-authored feedback request comment, clears the claim, restores the claimed-from state, adds `patchbay:feedback-requested` and `patchbay:automation-blocked`, and emits events. The caller must own the active claim. Automation must skip items with `patchbay:feedback-requested` until the label is removed after user feedback has been handled.

`PATCH /items/{item_id}` is for item field updates and supports version safety. It is separate from workflow transitions.

Relationship mutations validate that both work items exist in the same project, the source and target differ, the relationship kind is non-empty after trimming, and the exact `(project, source, target, kind)` relationship is not already present. Mutations touch both source and target work items, emit item events for both sides, and publish item-change notifications for both item detail views.

Project memory writes require Patchbay agent attribution in the request body. `PUT /memory` rewrites the complete memory field; `POST /memory/append` appends to the existing memory. Both create `MemoryChanged` events containing the full post-write memory snapshot. Compaction deletes memory history events only; the current project memory remains on the project record.

## CrudKit Endpoints

CrudKit-generated routes are mounted under `/api` for ordinary admin resources:

- projects;
- work items;
- comments;
- agent tools;
- agent runs;
- automation rules;
- personalities;
- work item states;
- swim-lanes.

CrudKit is not used for custom workflow authority. Admin CRUD can inspect and maintain records, but workflow transitions should use the custom endpoints so server services apply Patchbay rules consistently.

Automation rule CRUD exposes the explicit run mutability and selected personality for work-consuming rules. Create and update requests validate storage values `mutating` and `read_only`; new custom rules default to `mutating` unless the operator chooses read-only. Consume-work create and update requests default a missing personality to the project `Default` personality and reject missing or cross-project personality references. Existing rules migrated from older schemas remain `mutating` until edited.

Personality CRUD is project-scoped. Create and update requests trim and require `name`, keep `personality_description` as free-form text, and enforce unique names within a project. Delete requests reject `Default` and reject any personality referenced by an automation rule.

## UI Form Endpoints

The Leptos UI uses server form handlers for operator actions such as:

- creating, updating, and deleting projects;
- updating project prompts, memory, and settings;
- toggling project auto-commit and updating project commit, revert, and mutable Git command policy;
- updating the independent read-only automation concurrency limit;
- creating, updating, moving, deleting, and commenting on work items;
- creating, updating, and deleting work item relationships from item detail pages;
- starting, stopping, and recovering automation;
- canceling an individual active automation run;
- cleaning up worktrees;
- opening workspace folders or fixed editor targets such as RustRover and VS Code;
- creating, updating, deleting, and queueing evaluations for automation rules;
- creating, updating, and deleting project personalities;
- discovering agent tools;
- picking folders on the local system.

These endpoints are UI integration points, not the stable agent-facing API.

Direct automation starts may include an explicit mutability value. Omitted mutability defaults to `mutating`; work-producing evaluations ignore run mutability because they do not launch agents. Automation status responses include aggregate running runs plus separate mutating/read-only running counts and the effective mutating allowance.

Hydrated UI controls that save data in the background may post to these same form handlers with an internal background-request marker. Those requests should return a non-navigating success response while ordinary form posts keep their redirect fallback.

Project system prompt form writes create `SystemPromptChanged` events containing the full post-write prompt snapshot. System prompt history compaction deletes only old prompt events; the current project system prompt remains on the project record.

## Errors

API errors should be explicit enough for the CLI to show actionable output. Important error classes include:

- missing project context;
- unknown project or item;
- unknown relationship;
- cross-project relationship target;
- relationship source and target are the same item;
- empty relationship kind;
- duplicate relationship;
- invalid state transition;
- item already claimed;
- caller does not own the claim;
- stale expected version;
- automation tool unavailable;
- read-only launch unsupported by the selected agent tool;
- automation concurrency limit reached for the requested mutability;
- missing or cross-project automation personality;
- personality delete rejected because it is `Default` or still referenced by automation;
- run log unavailable.

The server should prefer structured error responses over plain text so CLI and future clients can distinguish user errors from server failures.
