# Repository Guidelines

## Layout

Patchbay uses standalone root-level Rust 2024 crates and intentionally has no root `Cargo.toml` or Cargo workspace.

- `patchbay-server/`: Axum server, Leptos UI, domain services, SQLite persistence, automation supervisor, styles, assets, and browser tests.
- `patchbay-types/`: shared request/response DTOs and enum types.
- `patchbay-api-client/`: typed HTTP client for Patchbay JSON endpoints.
- `patchbay-cli/`: standalone agent-facing `patchbay` CLI binary; it relays to a running server and must not open SQLite.
- `dev-bin/patchbay`: tracked development shim that puts the CLI relay on `PATH` before installation.
- `crudkit/`: Git submodule used as a local dependency; do not put Patchbay workflow rules there.
- `design/`: product and architecture notes.

Keep Patchbay-specific claim, progress, finish, release, automation, and board behavior in Patchbay-owned server services and custom API endpoints, not CrudKit routes.

## Commands

Run commands from the repository root through `just`, which passes explicit `--manifest-path` values because there is no root Cargo workspace.

- `just fmt`: format all Patchbay crates.
- `just check`: check server, CLI, API client, and types crates.
- `just test`: run standard Rust tests for Patchbay crates.
- `just clippy`: run clippy with `--all-targets -- -D warnings` for Patchbay crates.
- `just verify`: run formatting, tests, and clippy.
- `just serve`: run the server with `.patchbay/patchbay.sqlite3` on `127.0.0.1:4000`.
- `just cli -- <args>`: run the API-relay CLI.
- `just browser-test`: run the ignored browser integration test; use `just browser-test-visible` for UI debugging.

Server-local overrides: `PATCHBAY_DATABASE`, `PATCHBAY_BIND`, `PATCHBAY_PROJECT`, and `PATCHBAY_WORKSPACE_IDE`.
Automation CLI override: `PATCHBAY_CLI_PATH`.

## Agent-Facing Contract

The Patchbay server is the only process that owns or writes the database. Agents interact with Patchbay through the `patchbay` CLI, and the CLI is an API relay to `PATCHBAY_API_URL`.

Patchbay-launched agents receive `PATCHBAY_API_URL`, `PATCHBAY_PROJECT`, `PATCHBAY_AGENT_ID`, and `PATCHBAY_CLAIMED_ITEM_ID`. For the claimed item, prompts should use short commands such as `patchbay item show --json`, `patchbay comment list --json`, `patchbay item progress --body ...`, `patchbay item finish --report ...`, and `patchbay item release --comment ...`.

Project memory is Patchbay-owned storage, not Codex internal memory. Agents should persist important run discoveries with `patchbay memory append --body ...` and use `patchbay memory set --body ...` only for intentional full rewrites; memory writes must go through the Patchbay CLI/API so they create attributed `MemoryChanged` events.

CLI context resolution must prefer explicit flags, then environment variables. Missing required project, agent, or claimed-item context must fail instead of creating implicit data.

## Style

Use Rustfmt defaults. Keep Rust names idiomatic: `snake_case` for modules, functions, and fields; `PascalCase` for types and Leptos components; `SCREAMING_SNAKE_CASE` for constants. Organize modules by domain behavior rather than generic buckets.

Do not edit generated `style/crudkit` or `style/leptonic` content unless regenerating from the upstream source intentionally. Put Patchbay-owned styling under `patchbay-server/style/app/`.

## Testing

Place focused unit tests near the code they exercise. Browser coverage lives in `patchbay-server/tests/browser_test.rs` and is ignored by default because it starts Patchbay and Chrome.

When changing workflow paths, cover project scoping, claim ownership, progress, release, finish, stale-claim recovery, and version-safety behavior. When changing CLI/API behavior, cover context resolution and server-backed endpoint behavior.

## Git And PRs

Do not infer a project-specific commit format from history. Use short imperative subjects, for example `Add API relay CLI`.

PRs should include a concise behavior summary, verification commands run, linked work item or issue when available, and screenshots or notes for UI changes. Call out schema, CLI, API, automation prompt, or agent-instruction changes explicitly.
