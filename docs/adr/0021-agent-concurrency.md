# Agent concurrency is a queue plus a process-wide slot pool

Review, expert, professor, triage and diagram spawns enter a FIFO queue in `App.pending_background_tasks`, bounded by `ai_hub.max_concurrent_reviews` (default 3, meaning an unset or zero value; the TUI picker offers 1–6 and the desktop apply path accepts 1–16), and `er_engine::agent_slots` is a process-wide counting semaphore acquired before the spawn so that simultaneous arena runs cannot multiply the process count. A burst of review actions has to queue — with no bound, each click forks another provider process. Queued tasks render as a cancellable "queued" pill; `poll_background_tasks` dispatches them as slots free.

Two paths acquire a slot today: the background review queue dispatch and arena reviewer rounds. `spawn_agent_prompt`, `card_ai_spawn`, `spawn_command` and `model_discovery::run_models_command` spawn without one.

## Consequences

- The cap bounds the queued review and arena paths only; nothing enforces it process-wide. Calling it global is what put a false guarantee into the docs and into the `agent_slots` module comment, where it read as covering every spawn. A spawn path added later is uncapped unless its author acquires a slot explicitly.
