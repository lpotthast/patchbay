# Workflows

Patchbay workflows are enforced by server services. The CLI and UI send intent; the server validates project scope, ownership, item state, and version safety.

## Claim

Claiming work assigns an eligible item to an agent.

Inputs include:

- project;
- agent id;
- desired source state, usually `open`.

The server chooses an unclaimed item from the requested state, skips items with `patchbay:automation-blocked` or `patchbay:feedback-requested`, records the source state in `patchbay:claimed-from-state`, marks the item `in_progress`, records claim ownership and timestamps, increments version, and emits workflow events. New claims capture the item's current `state` label as the release source and overwrite any stale `patchbay:claimed-from-state` label left on the item. Default automation requests the `open` state; user-defined automation selectors can target other labels but the blocked-label exclusion is implicit. `item claim` never defaults to `PATCHBAY_CLAIMED_ITEM_ID`.

Claimable items must also be unfinished. Finished items are closed even if an operator later changes their `state` label to a value that matches a queue claim or automation selector.

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

The server validates claim ownership, appends the optional release comment, clears active claim ownership, restores the `state` label to the value captured in `patchbay:claimed-from-state`, increments version, and emits events. Agent-facing releases also add `patchbay:automation-blocked` so the item is not picked up again until a user or agent intentionally removes that label. Stale-claim recovery and cancellation restore the source state without newly blocking automation.

## Request Feedback

Requesting feedback pauses a claimed item because the agent needs a user answer before continuing.

For the claimed item, launched agents normally run:

```text
patchbay item request-feedback --body "Which provider should this integration target?"
```

The server validates claim ownership, appends the feedback request as an agent comment, clears active claim ownership, restores the `state` label to the value captured in `patchbay:claimed-from-state`, removes the claim-source bookkeeping label, adds `patchbay:feedback-requested` and `patchbay:automation-blocked`, increments version, and emits events. Automation skips feedback-requested items until the label is removed after the user response is handled. A later successful claim clears the pending feedback label so it represents only an active feedback wait.

## Item Updates

General item edits use the item update endpoint, not workflow endpoints. Updates can change title, description, state, and per-item agent overrides.

A single item update request is applied as one versioned change, even when it edits both item fields and the state label. Version checks protect against overwriting newer item state. Workflow transitions still use dedicated operations because they contain additional business rules.

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

## Automation Rule Behavior

Automation rules either produce work items or consume work items. Work-consuming automation has an explicit run mutability, either `mutating` or `read_only`. The rule prompt tells the launched agent how to handle the claimed item, including whether the expected outcome is implementation, refinement, verification, review preparation, or another project-specific workflow.

When a launched agent exits successfully while its item is still claimed, Patchbay releases the temporary claim back to the claimed-from state without adding `patchbay:automation-blocked`. This lets prompt-directed metadata, refinement, or verification consumers leave the underlying implementation work available for later automation. Failed runs still release with automation blocked so a broken prompt, missing context, or sandbox failure does not loop indefinitely. An agent can call `patchbay item request-feedback --body ...` when it needs a user answer before work can continue; it can call `patchbay item release --comment ...` for technical blockers or handoffs that need human triage but are not a concrete feedback request.

Patchbay ships editable default consumers for label-routed story preparation:

- a read-only refiner for items labeled `needs-refinement`;
- a read-only verifier for items labeled `needs-verification`.

Their prompts tell agents not to implement the work and not to call `patchbay item finish` for successful refinement or verification. The verifier may move an unnecessary item to a terminal workflow state only when that state is already evident from the project's user-defined workflow vocabulary; Patchbay does not hardcode a universal state value for that instruction.

Review-style work that should not run automatically is modeled as work-producing automation: a manual evaluation creates a review item with the expensive prompt, and a work-consuming automation can later run an agent against that item.

For Codex-backed launches, Patchbay prepares a project-specific Codex home before the run starts. The project home contains generated Codex config and rules derived from project settings, while shared Codex auth and skills are linked from Patchbay's shared managed Codex home when present. The run sets `CODEX_HOME` and `CODEX_SQLITE_HOME` to that project home so settings, rules, logs, sessions, and SQLite state are isolated per project.

Codex app-server stream transport interruptions that are consistent with host sleep, reconnect, broken pipe, or timeout behavior are recoverable. Patchbay keeps the run in `running`, keeps any claimed work item claimed, appends a concise recovery note to the active run output, and restarts Codex against the same persisted thread before asking it to continue. Recovery is bounded by an explicit retry limit. Explicit operator cancellation, project automation stop, server shutdown cancellation, the automation timeout, and non-retryable Codex turn failures remain terminal and continue to use the existing cancellation or failure release behavior.

## Automation Concurrency

Mutating and read-only runs use independent admission limits. A mutating run can start only when the running mutating count is below the workspace-constrained code-edit allowance derived from `max_code_edit_agents`; current-branch projects still cap that allowance at one. A read-only run can start only when the running read-only count is below `max_read_only_agents`; setting the read-only limit to zero disables read-only automation admission. Running read-only runs do not consume mutating slots, and running mutating runs do not consume read-only slots.

Queued automation evaluation and work-item polling evaluate admission against the candidate trigger's mutability. Status views expose both running counts and the effective mutating allowance so skipped or rejected starts can explain which limit was reached.

## Workspaces

Project settings choose the workspace policy:

- current branch;
- dedicated Git branch;
- Git worktree.

When worktrees or branches are used, run records capture the working directory, branch, and cleanup status. Cleanup can be manual or automatic after successful runs, depending on project settings.

Read-only runs do not allocate isolated branches or worktrees. They use the project checkout as their working directory with a read-only Codex sandbox and a read-only sandbox policy with network access enabled. Read-only Codex launches ignore project writable-root settings and project sandbox mode because the run mutability requires no project writable roots.

## Commit And Revert Policy

Project settings define an automation commit policy. `auto_commit` defaults to on and controls whether current-branch mutating runs are instructed to commit completed work before finishing. Agents generate the commit message from the completed diff and follow the project commit standard text when it is configured, otherwise they infer the repository's existing commit style.

Current-branch runs are instructed to inspect the initial git status, commit completed work only when auto-commit is enabled, and revert their own changes before releasing incomplete work. The current-branch failure revert strategy defaults to manual revert and can be changed to Git reset for projects that intentionally allow that more destructive cleanup path.

Git branch and Git worktree runs are always instructed to commit before ending the run. If the work is incomplete in those modes, agents commit useful partial work and release the item with an explanation instead of reverting the workspace, because the isolated branch or worktree preserves context for follow-up work without polluting the base workspace.

Patchbay records the run-level commit requirement and final commit outcome. The server captures the workspace Git state before launching the agent and compares it with the state after the agent process exits. Runs record created commit SHA(s), `skipped_no_changes` when no new commit or workspace change was detected, `skipped_no_git_repo` when the workspace is not a Git repository, and `missing_required` when a required commit was absent while new uncommitted changes remained. Completed agent processes with `missing_required` are marked as failed at the run level; the server records this without rewriting item history that the agent already reported through workflow commands.

Project settings also define the mutable Git command policy. New and migrated projects allow `git add`, `git commit`, `git push`, and `git reset` by default. `git commit` must use `--no-verify`; Patchbay's Git guard injects it when omitted and rejects `--verify`. Pushes must not be force, mirror, prune, delete, empty-source delete-refspec, or `+ref` pushes. `git reset --hard` is allowed only when the hard-reset policy allows it for isolated Git branch or Git worktree runs; it is blocked for current-branch runs by default.

Patchbay expresses the broad allow-list through generated Codex rules in the project Codex home. A run-specific `git` shim remains necessary for argument checks that prefix rules cannot express, such as a force-push flag appearing after the remote name. The generated prompt includes the effective Git commands expected to work for the run.

Read-only runs always receive a run-level Git policy with `add`, `commit`, `push`, and `reset` disabled regardless of the project's mutating Git policy. They have no commit requirement, do not request pull requests, and record commit handling as not required. The generated prompt states that project files may not be edited, mutable Git commands are unavailable, no commit is required, and sandbox or Git blockers should be reported instead of worked around. Read-only runs may still update Patchbay-owned metadata through authorized CLI/API calls when their prompt asks for that work.

## Stale Claims

Projects define a stale-claim timeout. Server maintenance can recover expired claims by clearing ownership and making the item available again.

Claim recovery is a server workflow. Agents should release work explicitly when they cannot continue, but they do not perform database maintenance themselves.

## Run Logs

Automation output is captured by the server and exposed through run-log endpoints and UI routes. While a run is active, run-log views should use the in-memory session output so operators can inspect intermediate output before the persisted log file is written. Agents and tools should request logs through the API instead of reading log paths directly.

## Pull Requests

When project settings request pull request creation, successful mutating automation can run the configured GitHub CLI flow from the prepared workspace and record the resulting PR URL on the run.

Pull request creation is a server-side post-run operation. Failure to create a PR should be recorded on the run without rewriting the completed item state unless server policy requires it.
