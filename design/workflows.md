# Workflows

Patchbay workflows are enforced by server services. The CLI and UI send intent; the server validates project scope, ownership, item state, and version safety.

## Claim

Claiming work assigns an eligible item to an agent.

Inputs include:

- project;
- agent id;
- desired source state, usually `open`.

The server chooses a claimable item, marks it `in_progress`, records claim ownership and timestamps, increments version, and emits workflow events. `item claim` never defaults to `PATCHBAY_CLAIMED_ITEM_ID`.

If no eligible item exists, the API reports that condition without creating implicit work.

## Progress

Progress records an agent-authored status update on an item.

For the claimed item, launched agents normally run:

```text
patchbay item progress --body "Implemented parser split."
```

The server verifies that the item belongs to the project and that the caller can update the item. It then appends a comment, records an event, and updates item metadata.

## Finish

Finishing work records a completion report and closes the active item.

For the claimed item, launched agents normally run:

```text
patchbay item finish --report "Done. Verified with cargo test."
```

The server validates claim ownership, appends the completion report, marks the item `done`, clears active claim ownership, records finish metadata, increments version, and emits events.

## Release

Releasing work returns a claimed item to the pool without marking it done.

For the claimed item, launched agents normally run:

```text
patchbay item release --comment "Blocked by missing credentials."
```

The server validates claim ownership, appends the optional release comment, clears active claim ownership, returns the item to an available state, increments version, and emits events.

## Item Updates

General item edits use the item update endpoint, not workflow endpoints. Updates can change title, description, state, automation eligibility, and per-item agent overrides.

Version checks protect against overwriting newer item state. Workflow transitions still use dedicated operations because they contain additional business rules.

## Automation Launch

When Patchbay launches an agent, it:

1. resolves an agent-facing CLI path;
2. prepends the CLI directory to `PATH`;
3. sets `PATCHBAY_API_URL`;
4. sets `PATCHBAY_PROJECT`;
5. sets `PATCHBAY_AGENT_ID` as `patchbay-run-<run-id>`;
6. sets `PATCHBAY_CLAIMED_ITEM_ID` when the run has claimed work;
7. omits `PATCHBAY_DATABASE`;
8. omits database paths from the prompt.

The prompt tells the agent:

```text
Use the `patchbay` CLI for Patchbay work state.
The CLI is available on PATH.
PATCHBAY_PROJECT, PATCHBAY_AGENT_ID, and PATCHBAY_CLAIMED_ITEM_ID are already set.
For the claimed item, omit project, agent, and item id arguments unless intentionally addressing another item.
```

## Automation Modes

Patchbay supports automation modes for code-editing and review-style work. Execute and refinement modes claim work. Review-style runs can inspect and report without taking the same claim path.

Mode-specific launch details are server policy. Agents should rely on the prepared prompt and environment, not infer policy from local files.

## Workspaces

Project settings choose the workspace policy:

- current branch;
- dedicated Git branch;
- Git worktree.

When worktrees or branches are used, run records capture the working directory, branch, and cleanup status. Cleanup can be manual or automatic after successful runs, depending on project settings.

## Stale Claims

Projects define a stale-claim timeout. Server maintenance can recover expired claims by clearing ownership and making the item available again.

Claim recovery is a server workflow. Agents should release work explicitly when they cannot continue, but they do not perform database maintenance themselves.

## Run Logs

Automation output is captured by the server and exposed through run-log endpoints and UI routes. Agents and tools should request logs through the API instead of reading log paths directly.

## Pull Requests

When project settings request pull request creation, successful automation can run the configured GitHub CLI flow from the prepared workspace and record the resulting PR URL on the run.

Pull request creation is a server-side post-run operation. Failure to create a PR should be recorded on the run without rewriting the completed item state unless server policy requires it.

