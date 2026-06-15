# Patchbay Agent Instructions

`patchbay` (a CLI, available on PATH) is the source of truth for work state and project memory.

## Prepared Context

Patchbay-launched agents receive:

```sh
PATCHBAY_API_URL=<api-url>
PATCHBAY_PROJECT=<project-name>
PATCHBAY_AGENT_ID=patchbay-run-<run-id>
PATCHBAY_CLAIMED_ITEM_ID=<item-id>
```

When `PATCHBAY_CLAIMED_ITEM_ID` is set, commands taking an `[item-id]` default to that id; omit the item id for normal claimed-item work. `item list`, `item create`, and `item claim` do not use the claimed item. Use an explicit item id only when intentionally addressing another item. Use `--project`, `--agent`, or `--api-url` only when deliberately overriding the prepared context.

## CLI Quick Reference

Work item and comment commands:

```text
patchbay item list [--state open|in_progress|done] [--json]
patchbay item show [item-id] [--json]
patchbay item create --title "..." --description "..." [--unclaimable] [--agent-model MODEL] [--agent-reasoning-effort none|minimal|low|medium|high|xhigh] [--json]
patchbay item update [item-id] [--title "..."] [--description "..."] [--state open|in_progress|done] [--automation-claimable true|false] [--agent-model MODEL] [--clear-agent-model] [--agent-reasoning-effort none|minimal|low|medium|high|xhigh] [--clear-agent-reasoning-effort] [--expect-version N] [--json]
patchbay item claim [--state open] [--json]
patchbay item progress [item-id] --body "..." [--json]
patchbay item finish [item-id] --report "..." [--json]
patchbay item release [item-id] [--comment "..."] [--json]
patchbay item watch [item-id] [--since-version N] [--json]
patchbay comment list [item-id] [--json]
patchbay comment add [item-id] --body "..." [--author "..."] [--author-type user|agent|system] [--json]
```

Project memory commands:

```text
patchbay memory show [--json]
patchbay memory history [--json]
patchbay memory append --body "..." [--json]
patchbay memory set --body "..." [--json]
```

Project memory is tracked through Patchbay, not through Codex internal memory or any other assistant memory feature. The generated prompt includes the full project memory snapshot for this run in its Project Memory section. Use `memory append` for important facts future agents should receive. Use `memory set` only for intentional full rewrites. Memory writes create attributed `MemoryChanged` events.

Automation visibility:

```text
patchbay automation runs [--limit N] [--json]
patchbay automation log <run-id> [--json]
```

## Workflow

You MUST perform these calls when: progress is made, the task is finished or cannot be finished.

```sh
patchbay item progress --body "Short progress note."
patchbay item finish --report "Done. Summary of changes and verification."
patchbay item release --comment "Why work is being stopped or handed back."
```

## Rules

- Treat the claimed Patchbay item as the current work contract.
- Re-read the item and comments before finishing because humans may edit work while you run.
- Keep progress comments concise and specific.
- Do not finish unless the requested work is complete or the final report explains why no code change was needed.
- If verification could not be run, say so in the finish report or release comment.
- If another worker owns an item or the server rejects a transition, do not bypass it with generic updates.
