# Agent concurrency is a queue plus a process-wide slot pool

Review, expert, professor, triage and diagram spawns enter a FIFO queue in `App.pending_background_tasks`, bounded by `ai_hub.max_concurrent_reviews` (default 3, meaning an unset or zero value; the TUI picker offers 1–6 and the desktop apply path accepts 1–16), and `er_engine::agent_slots` is a process-wide counting semaphore acquired before the spawn so that simultaneous arena runs cannot multiply the process count. A burst of review actions has to queue — with no bound, each click forks another provider process. Queued tasks render as a cancellable "queued" pill; `poll_background_tasks` dispatches them as slots free.

Coverage follows acquisition. Every path that spawns a review agent acquires a slot, so the cap holds for the work it is meant to bound. It is not a claim about subprocesses in general: a path that spawns something other than a review agent takes no slot, and a path added later is uncapped unless its author acquires one.

## Consequences

- The bound is a property of the call sites that acquire, not something the pool enforces by construction. A spawn path added later inherits no cap.
