# The task host and the run planners live in er-service

## Context

Phase 5 moved the desktop's agent runs off the App, but onto the desktop:
the task host that queues them under the agent cap (ADR 0021), the planners
that read a tab's content and prepare the bucket, and the job rows the page
lists all took the desktop's state. The TUI must run the same lenses on the
same buckets (ADR 0073), cannot depend on er-desktop, and may not plan a run
on its loop, because planning reads git and writes `diff-tmp`.

## Decision

The task host, the planners every front end runs, and the job-rows builder
live in er-service, beside `crates/er-service/src/tasks.rs`. The host takes a
small hooks value per call in place of a front end's state: the settings view's config, a way to tell the person, and
a call after a task finishes. It keeps each bucket's output lines after the
task that wrote them leaves the list, as a TUI tab's log did. `ServiceHost`
holds one app-wide host, sends `TasksChanged` on each row change, and plans a
run or builds a flow command on a worker thread of its own, so a caller gets
a receiver at once.

What only the desktop has stays in er-desktop: the notices and the inbox
(behind the hooks), sidebar triage, the card asks, the diagram, the scoped
review and a note sent to the agent.

## Considered Options

- **A second host in er-tui, copied from the desktop's.** Two copies of the
  cap, the queue and the run rules, and a TUI that writes the bucket, which
  the TUI's source scan forbids.
- **A workspace actor command per run kind.** Sidebar triage runs on a PR no
  tab shows, so no actor holds it, and the cap is app-wide, not per
  workspace.

## Consequences

- A planner reads only the content it is handed. The tab is captured when
  the run is asked for, so a tab switch while it plans cannot move it.
- A finished task's row still leaves the list after its window; its lines do
  not.
