# Agent tasks live in er-service

## Context

Phase 5 moved the desktop's agent runs off the App, but onto the desktop:
the task host that queues them under the agent cap (ADR 0021) and the job
rows the page lists both took the desktop's state, and so did the planners
that read a tab's content and prepare the bucket. The TUI must queue, plan
and list the same tasks (ADR 0073), cannot depend on er-desktop, and may not
plan a run on its loop, because planning reads git and writes `diff-tmp`.

## Decision

The task host, the job-rows builder and the planners every front end runs
live in er-service (`crates/er-service/src/tasks.rs`,
`crates/er-service/src/job_rows.rs`, `crates/er-service/src/runs.rs`).
The host takes a small hooks value per call in place of a front end's state:
the ports, the settings view's config, a way to tell the person, and a call
after a task finishes. It keeps each bucket's output lines after the task
that wrote them leaves the list, as a TUI tab's log did. `ServiceHost` holds
one app-wide host and sends `TasksChanged` on each row change and
`TaskNotice` for each message. It plans a run, or builds a flow command, on a
worker thread of its own, so a caller gets a receiver at once.

What only the desktop has stays in er-desktop, behind the hooks: the
notices, the inbox and the risk-rules reload after an importance run; and
the runs only it has: the diagram, the scoped review and a note sent to the
agent.

## Considered Options

- **A second host in er-tui, copied from the desktop's.** Two copies of the
  cap, the queue and the run rules, which would drift, and a TUI that writes
  the bucket.
- **A workspace actor command per task.** Sidebar triage runs on a PR no tab
  shows, so no actor holds it, and the cap is app-wide, not per workspace.

## Consequences

- A finished task's row still leaves the list after its window; its lines do
  not.
- A planner reads only the content it is handed. The tab is captured when
  the run is asked for, so a tab switch while it plans cannot move it.
- One worker plans for the whole app in turn, so a slow fetch delays the
  next plan, never the caller.
