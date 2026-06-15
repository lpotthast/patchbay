# Patchbay Design Overview

Patchbay coordinates software work across a local project, a server-owned work item database, a web UI, and launched coding agents. The server is the source of truth for persistence and workflow rules. Agents use the `patchbay` CLI as an HTTP relay to the server; they do not open SQLite or write Patchbay state directly.

## Core Invariants

- The Patchbay server is the only process that owns or writes the database.
- Agent-facing commands go through the standalone `patchbay` CLI.
- The agent-facing CLI calls the Patchbay JSON API and never opens SQLite.
- Patchbay-launched agents receive a prepared environment and should normally omit repeated project, agent, and claimed item arguments.
- Server-side workflow rules are authoritative for project scope, ownership claims, item state, and version safety.

## Document Map

- [architecture.md](architecture.md): process boundaries, crate layout, storage ownership, and CrudKit usage.
- [data-model.md](data-model.md): projects, work items, comments, runs, triggers, events, and settings.
- [api.md](api.md): custom JSON endpoints, UI form endpoints, streaming endpoints, and CrudKit boundaries.
- [cli.md](cli.md): standalone CLI contract, context resolution, commands, and development shim.
- [workflows.md](workflows.md): claim, progress, finish, release, automation launch, triggers, stale claims, and run logs.
- [ui.md](ui.md): Leptos routes, admin surfaces, live workflow visibility, and browser coverage.

## Repository Shape

Patchbay uses root-level Rust crates and intentionally has no root `Cargo.toml` workspace:

```text
patchbay-server/       server, SSR app, storage, automation, operator CLI
patchbay-types/        shared request and response DTOs
patchbay-api-client/   typed HTTP client
patchbay-cli/          standalone agent-facing CLI binary named patchbay
crudkit/               local CrudKit submodule dependency
dev-bin/patchbay       tracked development shim for the agent-facing CLI
```

The absence of a root workspace keeps the Patchbay crates independent from the `crudkit/` submodule workspace and avoids workspace dependency inheritance across repository boundaries. Repository-level `just` recipes call each crate with explicit `--manifest-path` values.

## Actors

- Human operators use the web UI and trusted server/operator commands.
- Patchbay automation launches coding agents with a prepared environment.
- Agents use only the agent-facing `patchbay` CLI for Patchbay work state.
- The server enforces all workflow transitions and owns the SQLite database.

## Design Boundary

CrudKit accelerates ordinary admin and CRUD surfaces such as projects, work items, comments, agent tools, agent runs, and automation triggers. Patchbay-specific workflow behavior remains custom: claim, progress, finish, release, stale-claim recovery, automation launch, run logs, live events, and board-oriented workflow views.

