# Agent tasks live in er-service

## Context

Phase 5 moved the desktop's agent runs off the App, but onto the desktop:
the task host that queues them under the agent cap (ADR 0021) and the job
rows the page lists both took the desktop's state. The TUI must queue and
list the same tasks (ADR 0073), and it cannot depend on er-desktop.

## Decision

The task host and the job-rows builder live in er-service
(`crates/er-service/src/tasks.rs`, `crates/er-service/src/job_rows.rs`).
The host takes a small hooks value per call in place of a front end's state:
the ports, the settings view's config, a way to tell the person, and a call
after a task finishes. It keeps each bucket's output lines after the task
that wrote them leaves the list, as a TUI tab's log did. `ServiceHost` holds
one app-wide host and sends `TasksChanged` on each row change and
`TaskNotice` for each message.

What only the desktop has stays in er-desktop, behind the hooks: the
notices, the inbox and the risk-rules reload after an importance run.

## Considered Options

- **A second host in er-tui, copied from the desktop's.** Two copies of the
  cap and the queue, which would drift.
- **A workspace actor command per task.** Sidebar triage runs on a PR no tab
  shows, so no actor holds it, and the cap is app-wide, not per workspace.

## Consequences

- A finished task's row still leaves the list after its window; its lines do
  not.
