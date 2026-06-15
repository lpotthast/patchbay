# Architecture

Patchbay is a local-first Rust application with a server-rendered and hydrated Leptos UI. The server owns persistence, workflow state, automation launch, and the HTTP API. The standalone CLI is an API client for agents and tooling.

## Process Boundaries

- `patchbay-server` is the only process that opens the SQLite database.
- `patchbay-cli` resolves context, validates command shape, and calls `patchbay-api-client`.
- `patchbay-api-client` contains typed HTTP calls and error handling.
- `patchbay-types` contains shared DTOs, enum types, and request payloads.
- Launched agents never receive a database path and never use a database-opening CLI.

## Crate Responsibilities

### `patchbay-server`

The server crate contains:

- the Axum and Leptos application;
- SeaORM entities and migrations;
- storage initialization and database path handling;
- project, item, comment, automation, and event services;
- custom JSON API endpoints;
- CrudKit-backed admin endpoints;
- automation process launch and log capture;
- the trusted server/operator CLI.

The operator CLI in this crate may accept `--database` because it is part of the trusted server surface. It is not the agent-facing CLI.

### `patchbay-types`

This crate defines shared transport types for the API client and server. Examples include project views, work item views, comments, agent runs, automation triggers, workflow request payloads, and shared enum values.

Types in this crate describe the wire contract. Server-only persistence details stay in `patchbay-server`.

### `patchbay-api-client`

This crate provides typed HTTP methods for the custom JSON API. It is used by `patchbay-cli` and can be reused by future tooling. It does not know about SQLite, SeaORM, Leptos, or server internals.

### `patchbay-cli`

This crate builds the `patchbay` binary used by agents. It is intentionally small: parse command arguments, resolve context from flags and environment variables, call the typed API client, and print human or JSON output.

## Storage

Patchbay persists data in SQLite through the server crate. The default database path is under the user's Patchbay data directory, while repository development recipes pass `.patchbay/patchbay.sqlite3` explicitly.

Database writes must flow through server services. This keeps workflow checks in one process and prevents launched agents from bypassing ownership, state, project, or version rules.

## Server Routes

The server exposes three classes of routes:

- Leptos UI routes for operators.
- Custom Patchbay JSON API routes under `/api/projects/...`.
- CrudKit-generated API routes under `/api` for ordinary admin resources.

Custom Patchbay workflow endpoints are not CrudKit endpoints. CrudKit remains an admin accelerator, not the authority for claim, finish, release, automation, or live workflow behavior.

## Development Commands

The repository-level `Justfile` uses explicit crate manifests because there is no root workspace. Common commands are:

```text
just fmt
just check
just test
just clippy
just verify
just serve
just cli -- item list --json
just browser-test
```

`just serve` runs the server with the repository-local database and default bind address. `dev-bin/patchbay` is the tracked development shim that runs `patchbay-cli` before the binary is installed globally.

