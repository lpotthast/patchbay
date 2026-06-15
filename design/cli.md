# CLI Design

Patchbay has two command surfaces:

- the standalone agent-facing `patchbay` binary from `patchbay-cli`;
- trusted server/operator commands in `patchbay-server`.

Only the standalone `patchbay` binary is part of the agent contract.

## Agent-Facing Contract

Launched agents are instructed to use:

```text
patchbay item show --json
patchbay comment list --json
patchbay item progress --body "..."
patchbay item finish --report "..."
patchbay item release --comment "..."
patchbay memory append --body "Important project fact to remember."
```

For follow-up items or explicit cross-item work, item ids remain available:

```text
patchbay item show 124 --json
patchbay comment list 124 --json
patchbay item progress 124 --body "Updated follow-up context."
```

Agents should omit project, agent, and claimed item arguments for the claimed item because Patchbay sets the environment before launch.

## Context Resolution

The standalone CLI resolves context in this order:

- API URL: `--api-url`, `PATCHBAY_API_URL`, `PATCHBAY_URL`, then the default local URL.
- Project: `--project`, then `PATCHBAY_PROJECT`.
- Agent: `--agent`, then `PATCHBAY_AGENT_ID`.
- Claimed item: explicit positional item id, then `PATCHBAY_CLAIMED_ITEM_ID`.

Commands that choose or create work do not default to the claimed item. This includes:

- `item claim`;
- `item list`;
- `item create`.

Commands that operate on an existing item accept an optional item id and may default to the claimed item:

- `item show [item-id]`;
- `item update [item-id]`;
- `item progress [item-id]`;
- `item finish [item-id]`;
- `item release [item-id]`;
- `item watch [item-id]`;
- `comment list [item-id]`;
- `comment add [item-id]`.

## Commands

Work item commands:

```text
patchbay item list [--state <state>] [--json]
patchbay item show [item-id] [--json]
patchbay item create --title "..." --description "..." [--json]
patchbay item update [item-id] [options] [--json]
patchbay item claim [--state open] [--json]
patchbay item progress [item-id] --body "..." [--json]
patchbay item finish [item-id] --report "..." [--json]
patchbay item release [item-id] [--comment "..."] [--json]
patchbay item watch [item-id] [--since-version <n>] [--json]
```

Comment commands:

```text
patchbay comment list [item-id] [--json]
patchbay comment add [item-id] --body "..." [--author "..."] [--author-type user|agent|system] [--json]
```

Memory commands:

```text
patchbay memory show [--json]
patchbay memory history [--json]
patchbay memory append --body "..." [--json]
patchbay memory set --body "..." [--json]
```

`memory append` and `memory set` require project and agent context. They write through the Patchbay API, never through Codex internal memory, and create attributed `MemoryChanged` events.

Automation commands:

```text
patchbay automation runs [--limit <n>] [--json]
patchbay automation log <run-id> [--json]
```

Global flags:

```text
--api-url <url>
--project <project>
--agent <agent-id>
```

## Development Shim

Before the CLI is installed, development uses the tracked shim:

```text
dev-bin/patchbay
```

The shim runs the root-level `patchbay-cli` crate with Cargo. Patchbay development mode can configure this path as the agent-facing CLI and prepend `dev-bin` to `PATH`.

The shim is tracked in Git so automation prompts and local development do not depend on an ignored file.

When `CARGO_TARGET_DIR` is not already set, the shim builds into a writable temporary target directory. The shim is a Cargo launcher, not the installed CLI binary, so Cargo otherwise writes build outputs and lock files under the Patchbay source checkout. Patchbay-launched Codex agents run with Codex app-server's workspace-write sandbox rooted at the target project workspace; they should not need write access to the Patchbay checkout just to call the API relay. `PATCHBAY_CLI_TARGET_DIR` can override the shim's default temp target directory.

## Server Operator CLI

The server crate also contains trusted commands for running the server and operating local state. That surface may accept database paths and perform privileged maintenance. It must not be presented as the normal agent-facing Patchbay interface.
