# TODO

## Workflows

It is not yet clear what the best "automated workflow" looks like. Here are a few ideas:

### Follow-up work

Always instruct agents to create "follow-up work items" for things they deem necessary, important or otherwise
meaningful.
This can go hand-in-hand with other concepts, like "idea"s, described below.

This allows for real "endless" executions. Whenever one agent comes up with something, it is shortly executed after the
initial agent finished in a new session.

### Idea

An additional "idea" swim-lane exists. Work items in this lane are never automatically picked up by automation.
It could also be named "icebox" or "backlog", although "idea" has more of a "volatile" meaning to it..

Both human operators and agent can create ideas.

### Labels

Each work item can have a set of unique labels. Labels are basically a `HashSet<String, Option<String>>`.

Allowing for labels like: "bug", "severity=high", "easy", "env=prod", ...

Labels can be edited by human operators and agents. Agents can add and remove labels while working on an item.

Triggers can be configured to only act when certain label rules are met.

Special system-prompt extensions can be passed to agents based on item labels.

### Refinement

A user might not flesh out a work item that thoroughly.

We can add a trigger that listens/watches for work-items having the "needs-refinement" label, claims the item and lets
an agent refine the requirements and create a full implementation plan.

This could require back and forth with the user, which is allowed by the item comments system.

This requires (also in general) ALL agents to be informed that they are running "headless" / "asynchronous" and must not
expect immediate user feedback and that they can request user feedback through work item comments and a special "
needs-feedback" label.

"needs-feedback" labeled items are never picked up by automation.

Commenting a "needs-feedback" labeled item removes that label automatically.

With the `Main automation is also a "trigger"` feature described below, users could set all of this up themselves, by
telling the main automation (maybe we should rename "Triggers" -> "Automations") to ignore "needs-feedback" entries.

### Main automation is also a "trigger"

Currently, pressing "Start" starts the automatic pickup of "open" work items. But that automatic pickup is not user
configurable. It would be a benefit if that would be a simple "when=always" kind-of trigger entry, having no special
rules for when to run.

### Related-to-other

Allow work items to reference each other.

Case: "Work breaker" -> If work too big, split into multiple smaller new work items. Let them all reference the current
work item (as their what? "parent"?). Append "[split]" to current work item name and finish it off.
