# Agent concurrency is a queue plus a process-wide slot pool

Review, expert, professor, triage and diagram spawns enter a FIFO queue in `App.pending_background_tasks`, bounded by `ai_hub.max_concurrent_reviews` (default 3, meaning an unset or zero value; the TUI picker offers 1–6 and the desktop apply path accepts 1–16), and `er_agent::agent_slots` is a process-wide counting semaphore acquired before the spawn so that simultaneous arena runs cannot multiply the process count. A burst of review actions has to queue — with no bound, each click forks another provider process. Queued tasks render as a cancellable "queued" pill; `poll_background_tasks` dispatches them as slots free.

Coverage follows acquisition. Every path that spawns a review agent acquires a slot, so the cap holds for the work it is meant to bound. It is not a claim about subprocesses in general: a path that spawns something other than a review agent takes no slot.

The pool is private to `er-agent` (P33b). `JobRunner` takes the slot for each attempt and releases it when that attempt ends, so the acquisition and the spawn cannot drift apart; the engine no longer holds a slot itself. The acquire functions are not reachable from another crate, and the `er::agent_spawn` gate reports a raw `Command::new` outside `er-agent` and the OS-launch modules.

## Consequences

- The bound is a property of the runner, not of each call site. A spawn added through `JobRunner` inherits the cap; one that bypasses the runner is a gate finding, not an uncapped path.
- A retry holds one slot per attempt, never two: the attempt's guard drops before the backoff, and the next attempt acquires again.
