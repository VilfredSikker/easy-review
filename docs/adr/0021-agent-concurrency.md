# Agent concurrency is a queue plus a process-wide slot pool

Review, expert, professor, triage and diagram spawns enter a FIFO queue in `App.pending_background_tasks`, bounded by `ai_hub.max_concurrent_reviews` (default 3, meaning an unset or zero value; the TUI picker offers 1–6 and the desktop apply path accepts 1–16), and `er_engine::agent_slots` is a process-wide counting semaphore acquired before the spawn so that simultaneous arena runs cannot multiply the process count. A burst of review actions has to queue — with no bound, each click forks another provider process. Queued tasks render as a cancellable "queued" pill; `poll_background_tasks` dispatches them as slots free.

Every path that spawns a review agent acquires a slot: the background review queue dispatch, arena reviewer rounds, **the arena arbiter**, the AI Hub prompt (`App::spawn_agent_prompt`), desktop card AI (`run_card_ai_subprocess`) and a configured shell command (`App::spawn_command`, where the summary agent runs). The arbiter is the easy one to miss — it emits its progress under the same reviewer id as a round, so a reader folds it into "arena reviewer rounds" and concludes their cap covers it. It acquires on its own, because `start_arena_batch` starts one run per group, each with its own arbiter.

Two subprocess paths take no slot, and neither runs a review agent: `model_discovery::run_models_command`, a model-listing probe bounded by its own 10s timeout, and the desktop PTY in `er-desktop`, which spawns `$SHELL`.

## Consequences

- A spawn path is capped only if it acquires a slot; one added later is uncapped unless its author acquires one. The cap bounds review agents process-wide as a property of the call sites that acquire, not because the pool enforces it by construction. An earlier revision of this ADR listed four of those paths as ungated, which an audit of the spawn sites against the tree no longer supported.
