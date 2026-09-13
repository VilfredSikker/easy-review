# AI Agent Runners & CPU-Hot Path Audit

Status: **Phases 0 and 1 complete. Phase 1's redraw gate is deferred with its
reason; Phases 2-4 not started.**

Phase 0 instrumentation has landed (`er-engine/src/agent_timing.rs`, plus call
sites in `agent_slots.rs`, `app/state/comments.rs`, `arena/adapter.rs`,
`arena/orchestrator.rs`) with two headless harnesses. Every claim below was
adversarially re-verified by a second pass that was told to refute it; where the
first pass was wrong the corrected statement is marked **[corrected]** and the
original is shown so the error stays visible.

## Context

`er` runs AI reviews by spawning an external CLI (`claude`, `codex`, `opencode`, …)
as an OS subprocess. Three problems prompted this audit:

1. **Wall-clock** — review / expert / professor / arena runs feel slow.
2. **UI jank** — the app stutters while agents run.
3. **Audit** — find and rank the CPU- and hardware-heavy paths.

Restructuring the execution model is in scope. Both UIs matter; shared fixes belong
in `er-engine`. Reference machine: M3 Pro, 11 cores (5P + 6E), 18 GB.

Three conclusions drive the recommendation:

- **Agent processes are not CPU-bound — measured.** One real provider call cost
  **59.58 s wall against 3.60 s CPU (2.22 user + 1.38 sys), 6.0 % utilisation**.
  Adding `rayon` or moving to async buys nothing here.
- **Arena wall-clock is dominated by sequential rounds, not by the concurrency
  cap — measured.** Rounds are barrier-joined, so the default 3-reviewer /
  3-round arena runs three serial waves. Verified below: three reviewers in one
  round cost one reviewer-latency, and rounds add linearly.
- **The cap does not actually cap everything.** **Six** spawn paths exist, not
  five **[corrected]**: four bypass both gates, and one long-running process
  holds a slot forever because nothing times out and running reviews cannot be
  cancelled.

Two details worth fixing while in here. The desktop settings clamp the cap to
`1..=16` and offer `1..=6` in its own cycle list — **both ranges belong to the
desktop**, not to a TUI/desktop disagreement **[corrected]**; the TUI Config Hub
has no `max_concurrent_reviews` item at all. And
`er-engine/src/agent_runtime.rs` (`resolve_invocation`, `build_argv`,
`decode_final_text`) has **no callers in the workspace** — dead exported surface
(`lib.rs:18`), confirmed at 1339 lines.

## How this was researched

Method, stated so the confidence level is legible:

- **Three parallel `Explore` subagents**, each with a separate brief: (a) trace the
  agent spawn path end to end, (b) find and rank CPU/hardware hotspots by cost,
  (c) map the async/concurrency/locking model across all crates and the frontend.
- **Direct reading by the author**, concurrently: all of `agent_slots.rs`, the spawn
  body in `comments.rs`, `arena/adapter.rs` `run_once`, `orchestrator.rs` rounds,
  `drain_agent_log`, `dispatch_pending_background_tasks`, and the config defaults.
- **An adversarial verification pass** (14 agents, one per claim group, each
  followed by a skeptic instructed to refute its verdict and to default to
  "refuted" when it could not independently confirm). This is where the
  **[corrected]** markers come from. It found real errors, including one whole
  missing spawn path and two hotspots that do not hold as described.
- **Phase 0 measurement** — see "Phase 0 results" below. Headless harnesses drive
  the real supervisor and the real spawn path with a fake provider, so the
  numbers are reproducible without a model call.

**What was not done:** the app itself has still never been run or profiled in
situ, and no agent was executed as part of the app. The CPU figure comes from
timing a provider CLI directly; the arena and slot figures come from headless
drivers of the same code paths with simulated latency. Phase 0's two
conclusions are therefore measured, but measured off-app.

Line numbers shift as code lands. The citations below were checked against the
tree at the time of the verification pass; the uncommitted Phase 0
instrumentation itself moved several `comments.rs` citations by 12-27 lines.

## What we found

### Six spawn sites, two gates, uneven coverage

| # | Path | Spawn site | Slot? | Queue? | Wait style |
|---|------|-----------|-------|--------|-----------|
| A | Background task (review / expert / tour / triage / professor / diagram) | `app/state/comments.rs:3081` | yes (`:2991`) | yes | piped, 2 reader threads, blocking `wait()` (`:3141`) |
| B | Tab-level command (`spawn_agent_prompt`) | `comments.rs:2462` | **no** | **no** | piped, 2 reader threads, blocking `wait()` (`:2530`) |
| C | Card AI (ask / validate / elaborate / finding reply) | `app/card_ai_spawn.rs:195` | **no** | **no** | `cmd.output()`, fully buffered |
| D | Arena reviewer / arbiter | `arena/adapter.rs:119` | reviewers yes (`orchestrator.rs:580`, `:718`); **arbiter no** (`:999`) | n/a | concurrent pipe readers, blocking `wait()` |
| E | Model discovery | `model_discovery.rs:76` | n/a | n/a | `try_wait()` loop, 10 s timeout |
| F | Configured shell command — the summary agent runs here **[corrected]** | `comments.rs:2036` → `sh -c` at `:2084` | **no** | **no** | piped, 2 reader threads, blocking `wait()` |

**[corrected]** The first pass listed five sites and called the enumeration
complete. `App::spawn_command` spawns `sh -c <configured command>` with no slot
and no queue, and the app documents it as a provider-CLI launcher: the
`[commands]` summary example is `claude --print -p '...'`. The provider-CLI
enumeration was complete; the ungated-path enumeration was not. (A seventh
subprocess path — the desktop PTY in `er-desktop/src/terminal.rs:32`, which
spawns `$SHELL` — is ungated but is not a provider CLI.)

Paths B, C and F honour neither gate, so repeated "Ask AI" / "Validate" clicks
spawn provider processes without bound. Only A and D respect
`max_concurrent_reviews`, and D only for its reviewers.

### The concurrency cap

`er-engine/src/agent_slots.rs` is a process-wide counting semaphore
(`Mutex<usize>` + `Condvar`, 200 ms `wait_timeout` for cancellation).
`DEFAULT_MAX_CONCURRENT_REVIEWS = 3` (`config/mod.rs:182`). The guard is RAII, so
the mutex is never held across a spawn or a wait.

Two independent, non-cooperating limiters read the same number:

1. the App-level FIFO `pending_background_tasks`, drained by
   `dispatch_pending_background_tasks` (`comments.rs:3394`) while
   `running_background_task_count() < cap`;
2. the process-wide slot pool, acquired at spawn time.

Limiter 1 counts only App-level tasks, so it cannot see arena reviewers holding the
same slots. It launches work, marks it `Running` in the UI, and the thread parks in
`acquire_blocking` — which has no cancel path and no timeout, so it waits forever.
During arena contention the UI shows agents "running" that have not started.

The cap binds only when the reviewer count exceeds it; see the measured runs
below.

### Arena: the wall-clock shape

`ARENA_ROUNDS_V1 = 3`, `MIN_QUORUM = 2`, `effective_arena_rounds` clamps 1–3
(`orchestrator.rs:27-41`); one reviewer forces one round. Per run:

- **Round 1** — one thread per reviewer (`:572`), each taking a slot (`:580`), then
  a **barrier join over all of them** (`:632`).
- **Rounds 2..=R** — one thread per active reviewer (`:712`), slot at `:718`,
  **barrier join** (`:767`).
- **Arbiter** — a single sequential `run_provider_json` on the supervisor thread
  (`:999`), **without a slot**.

A 3-reviewer / 3-round run is 9 reviewer processes + 1 arbiter, and costs roughly
`rounds × reviewer latency`. More reviewers do not shorten it; fewer rounds do.
Both halves of that are now measured.

### No timeouts, no cancellation

There is **no timeout on any agent process** — the only provider-CLI timeout in the
codebase is the 10 s one in `model_discovery.rs`. A hung `claude` hangs forever.

Retries exist only in the arena (`MAX_RETRIES = 2`, `adapter.rs:13`, with
`classify_error` for Transient / RateLimit / Fatal). The background, tab-level,
card and shell-command paths have none.

**A running background review cannot be cancelled.** `BackgroundTaskHandle`
(`app/state/background.rs:155-164`) holds only `result_rx` / `log_rx` / `recent_log`
— no `Child`. The child is moved into the worker thread and dropped after `wait()`;
`cancel_queued_background_task` (`comments.rs:3421`) removes only *queued* entries.
Arena cancel works because `ArenaRunHandle` keeps its `Child` handles
(`registry.rs:19-27`).

### Per-run and per-line cost

Per output line, the reader threads allocate ~4 `String`s: the transient `lines()`
result, a retained `line.clone()`, a trimmed copy for the UI entry, and a fresh
`command_name.to_string()` per entry.

At completion every run writes its **entire** stdout+stderr transcript to
`er_dir/debug-agent.log`, with no flag gating it. Peak memory holds it
three times: the `Vec<String>`, the `.join("\n")` copy, and the `format!` copy.

### Channels and log draining

The workspace has exactly one channel primitive: unbounded `std::sync::mpsc`. Zero
`sync_channel`. Producers never block or check depth.

`poll_background_tasks` drains with an unbounded `try_recv` loop and **clones each
entry into every matching tab's `agent_log`** plus the handle's `recent_log`, all
under the `App` mutex. The UI-side caps (5000 / 500) bound what is *kept*, not what
is *queued*. Log entries do not bump `desktop_revision`, so on the desktop they
drain only inside `poll_impl` (`commands.rs:9825`) — up to the 30 s safety-net
window.

### Locking and the results path

`er-desktop` serializes on `AppState.app: Arc<Mutex<App>>` — **120** literal
`app.lock()` occurrences in `commands.rs`, not 131 **[corrected]**: 119 call sites
plus the `snap!` macro definition at `:258` (137 if the `app_arc` / `app_state` /
multiline-alias forms are counted too). The polling daemons correctly use a
three-phase shape (`try_lock` → no lock → apply), and `lock_wait_ms` already
instruments contention.

The lock *is* held by:

- `snap_from` (`commands.rs:265`) — the whole snapshot build, with `sent_files`
  nested inside (the one ordering invariant: `app` → `sent_files`).
- `poll_impl` (`commands.rs:9819`) — across `drain_agent_log`, `check_commands`,
  `poll_background_tasks`, `check_ai_files_changed`, revision hashing, and the build.
- `reload_ai_state()` (`app/state/mod.rs:3324`), reached from `check_ai_files_changed`
  (`:3724`), doing a full sidecar re-read and `serde_json` parse. This is the results
  hot path and it runs under the mutex on the poll thread.
- `run_ai_review` builds the diff (`tab.raw_diff_for_review`, which shells to git)
  inside the lock (`commands.rs:3537-3556`).

`arena_start` is a **synchronous** `#[tauri::command]` (`arena_commands.rs:197`): it
runs on the Tauri main thread, takes the App lock (`:224`), and does git + file IO
before spawning the supervisor. It freezes the window on a large PR.

`run_blocking` covers 70 of 141 commands, enforced by a test (`commands.rs:11493`).
**67** sync commands remain, not 65 **[corrected]**; `config_commands.rs` is 7
sync + 3 async-with-`run_blocking`, not 10 sync. The guard test asserts a
hardcoded list of 50 command names plus 3 wrappers rather than coverage of all
141. `open_worktree` (`:5898`) opens a blocking native `FileDialog` inline.

### Ranked CPU hotspots

1. **Watch-event refresh does far more than re-parse.** **[corrected]** the call
   site: this is the desktop watch path, `er-desktop/src/main.rs:1444` (TUI
   equivalent `er-tui/src/main.rs:477`) → `refresh_diff_quick_with_unmark()`
   (`app/state/mod.rs:2531`, args `false, true, true` at `:2532`) →
   `refresh_diff_impl(false, true, true)`. The third arg is `true`, so despite the
   name each event runs `git diff` + `git merge-base` + `git ls-files --others`,
   reads every untracked file into memory, parses the full diff, computes **full
   SHA-256 per file** over the whole diff (`ai/loader.rs:56`), and calls
   `reload_ai_state()` **unconditionally** (`state/mod.rs:2913`), reading and parsing
   every sidecar (up to 10 MB each), then clears the mtime cache and re-stats every
   file. A sustained write stream re-arms the debouncer and fires this every 400 ms
   (desktop) / 200 ms (TUI). `state/mod.rs:3125` also still computes full SHA-256 for
   `branch_diff_hash` on quick refreshes, so the fast `DefaultHasher` path buys
   nothing in Branch mode.
2. **`git merge-base --is-ancestor` spawned once per local branch** every 60 s
   (`snapshot.rs:2782`, `:2897`) — 100+ spawns per meta refresh on a repo with 100
   branches. One `git branch --merged <base>` replaces them. Two qualifications
   from the verification pass: the second site is bounded by a `limit` argument
   (10 at the call site, `snapshot.rs:1566`), and both skip the spawns entirely
   when `skip_merged` is set (more than 10 worktrees, or an empty base).
3. **`sort_files_by_mtime` stats inside the comparator** (`state/mod.rs:4404`) —
   ~`2n log n` syscalls per refresh (500 files ≈ 9000 stats) plus a `format!` each.
   Only runs when the mtime-sort toggle is on; **[corrected]** that toggle is `m`
   (`er-tui/src/input/normal.rs:52-54`), not `Shift+R`.
4. **`git ls-remote origin <base>` every 60 s with no gate** on a loop whose
   siblings all have TTLs. **[corrected]** the invocation is at
   `er-desktop/src/main.rs:976-977`; `:951` is the loop spawn and `:952` the 60 s
   sleep.
5. **Syntax highlighting.** **[corrected — the TUI half does not hold as
   written]** the cited path was wrong: TUI highlighting lives in
   `crates/er-engine/src/highlight.rs`; `crates/er-tui/src/ui/highlight.rs` is a
   44-line adapter. The disk open is syntect-internal and happens only when the
   filename/extension lookup misses, not on every cache miss. What stands: the
   desktop funnels every request through one `workChain` promise
   (`highlightWorker.ts:157`) on a single worker (`highlightClient.ts:9`), so the
   "4 concurrent" cap is a queue on one worker; eviction sorts all 10 000 entries;
   a cache hit still clones every span per line per frame.
6. **TUI redraws unconditionally at 50 ms** (`er-tui/src/main.rs:435,438`) ≈ 20 fps
   while idle; tick-based timers still assume 100 ms, so "≈1s" jobs run at ~0.5 s and
   "≈5s" jobs at ~2.5 s, the latter spawning `git check-ignore`.
7. **Frontend geometry rebuild.** **[corrected — the self-trigger framing is
   wrong]** `onHeightChange` (`FlatDiffView.svelte:358`) returns early at `:361`
   when the measured height equals the recorded one, so the
   `ResizeObserver` → geometry → re-observe cascade converges instead of looping.
   What remains is cost, not a cycle: `baseGeometry` is `$derived` on
   `overlaySerial` and rebuilds all of `cumulativeOffsets`, and each measurement
   can allocate a `Map`.
8. **`SNAPSHOT_DIFF_LINE_BUDGET` is global, not per-viewport** (`snapshot.rs:282`) —
   spent first-come-first-served across `visible_files()`, landing independent of
   where the user is looking. It resets per `build_snapshot` call (global across
   files within a snapshot, not across snapshots), is overridable via
   `ER_DESKTOP_SNAPSHOT_LINE_BUDGET`, and the currently selected file is
   prioritised.

## Phase 0 results (measured)

Instruments: `er-engine/src/agent_timing.rs`, off unless `ER_AGENT_TIMING=1`.
Slot waits are recorded inside `SlotPool::acquire` so every spawn path is covered
without touching each one; per-run phase splits come from the spawn sites; rounds
are timed in `run_supervisor`. Harnesses:
`crates/er-engine/tests/arena_timing.rs` (full supervisor, fake provider) and the
`harness_measures_a_real_spawn` unit test in `arena/adapter.rs`.

### An agent is not CPU-bound

```
claude -p "Reply with exactly the word ok and nothing else."
59.58 real    2.22 user    1.38 sys          # 3.60 s CPU = 6.0 % utilisation
```

94 % of the process's life is blocked on the provider. This is the number that
retires any plan to parallelise the spawn path.

### Spawn overhead is negligible

Across 14 slot acquisitions the queue and spawn phases were sub-millisecond
(`queue_ms=0 spawn_ms=0`); `run_ms` held everything. Instrument sanity check: a
child sleeping 300 ms reports `spawn_ms=14 run_ms=353`.

**[corrected]** The `queue_ms=0` half of that means nothing on these
acquisitions. The arena takes the slot in the orchestrator (`orchestrator.rs:580`,
`:718`) and only then calls `run_once`, which starts the timer and immediately
marks slot-acquired (`arena/adapter.rs:113-116`) — so `queue_ms` is identically
zero there by construction, and the 2142 ms wait reported below is structurally
invisible to it. Only the background-task path measures queue properly — timer
started, then `acquire_blocking`, then `mark_slot_acquired`
(`comments.rs:3028`/`:3034`). The other instrumented sites mark immediately
because they take no slot at all: path F says so in as many words
(`comments.rs:2106`/`:2111`, "No slot and no queue"), and the arena path's slot
was taken by its caller. So `queue_ms=0` is structural on three of the four
instrumented sites and cannot support a general "the queue is negligible".

The `spawn_ms` half stands on its own — a child sleeping 300 ms reporting
`spawn_ms=14` is a real measurement — so "spawn overhead is negligible"
survives; "the queue is negligible" is not what this shows.

### Rounds set arena wall-clock; the cap binds only above itself

Fake provider sleeping a fixed 2 s per reviewer and 1 s for the arbiter, driven
through the real `start_arena_run` → `run_supervisor` path:

| Run | Reviewers | Rounds | Round wall | Total |
|-----|-----------|--------|-----------|-------|
| A | 3 | 1 | 2213 ms | 2.25 s |
| B | 3 | 2 | R1 2207 · R2 2086 · arbiter 1059 | 5.36 s |
| C | 5 | 1 | 4188 ms | 4.24 s |

- **Reviewers within a round are concurrent.** Three reviewers at 2 s each
  produced a 2213 ms round, not ~6 s.
- **Rounds are serial and additive.** 2207 + 2086 + 1059 = 5.35 s against 5.36 s
  measured.
- **The arbiter is serial addition** with no slot, and cannot overlap.
- **The cap binds only above itself.** Runs A and B (3 reviewers, cap 3) recorded
  **zero** blocked acquisitions. Run C recorded `blocked=2 max_wait_ms=2142` — a
  reviewer waiting exactly one reviewer-latency for a slot.

  **[corrected]** These numbers predate a tightening of the metric. `started`
  used to be taken before the pool's mutex, so mutex-acquisition time counted as
  slot wait, and a cancelled waiter recorded an acquisition it never got. Both
  are fixed; `acquires` now means grants and `blocked` means grants that waited
  ≥1 ms. The conclusion is unaffected — run C is 5 reviewers against a cap of 3,
  so exactly two must wait, and a 2142 ms wait is two orders of magnitude above
  any mutex hold. The absolute values would move slightly on a re-run.

At real provider latency (~60 s, measured above) the default 3-reviewer /
3-round arena is roughly four serial reviewer latencies plus the arbiter, and it
is already under the cap. Raising `max_concurrent_reviews` does nothing for it;
the round count is the only lever.

### Still unmeasured

- **The two-limiter starvation interaction.** Every run above had
  cap == reviewer count, so there was no contention to observe. Demonstrating
  that `dispatch_pending_background_tasks` launches work which then parks
  invisibly needs a background review and an arena overlapping.
- **In-situ behaviour.** No profile of the running app exists; these numbers
  come from headless drivers of the same code paths.

## Documentation drift

A sample of roughly ten claims across root `CLAUDE.md`, `crates/er-engine/src/CLAUDE.md`,
and `crates/er-engine/src/app/CLAUDE.md` was checked. Several are wrong, in four classes.

### Claims of a guarantee that does not hold

Worst class, because it misleads exactly the kind of reasoning this audit required.

| Doc | Claim | Reality |
|-----|-------|---------|
| root `CLAUDE.md:82` | `er_engine::agent_slots` is "acquired before **every** agent subprocess spawn" | Only **3** acquisition sites exist (`comments.rs:2991`, `orchestrator.rs:580`, `:718`). `spawn_agent_prompt`, card AI, `spawn_command` and the arena arbiter all spawn with no slot. **[corrected]** the ungated set is four paths, not three. |
| `agent_slots.rs:5` | "a single hard cap across every spawn path" | Same reality. **[corrected]** this wording is in the module doc, not root `CLAUDE.md` — the first pass attributed it to the wrong file. |

### Stale architecture descriptions

| Doc | Claim | Reality |
|-----|-------|---------|
| root `CLAUDE.md` | event loop "polls for keyboard input (100ms timeout)" | `er-tui/src/main.rs:438` polls at **50 ms**, and tick-count constants elsewhere still assume 100 ms |
| root `CLAUDE.md` | "No async runtime needed" | true for `er-engine`/`er-tui`; `er-desktop/Cargo.toml:24` declares `tokio` with `features = ["full"]`, and `er-mcp` runs `rt-multi-thread` |
| root `CLAUDE.md` | workspace is `er-engine`, `er-tui`, `er-desktop` + `desktop-ui` | `Cargo.toml:3` lists **five** members: adds `er-mcp` and `er-crap` |
| root `CLAUDE.md` | frontend "polls snapshots over Tauri IPC" | `app.svelte.ts:207` sets `pollIntervalMs = 30_000` as a safety net; the model is push-driven off an 80 ms revision watcher |

### Descriptions of removed features

`crates/er-engine/src/app/CLAUDE.md` documents `.er-config.toml` (repo root) as
configuring "features and watched files". Per-repo config was removed; config is
global-only. There is **no loading code left**, but stale user-facing strings still
reference the file. **[corrected]** there are **3** runtime strings, not five, and
they are in `app/state/mod.rs` — `:4951` (a doc comment), `:6216` and `:6311`
(the two `not_configured` strings), plus `er-tui/src/input/normal.rs:216`. Line
numbers here were wrong twice; those are current as of the Phase 1 branch.
`docs/index.html` also tells users settings persist to `.er-config.toml` (around
`:1204`, `:1268`, `:1272`), and so do `docs/config-reference.md:10,239` and
`docs/guide/configuration.html:24,183`. That is a live UX bug, not just doc drift:
the app directs users to edit a file that does nothing.

### Incomplete reference tables

- Root `CLAUDE.md`'s File Map lists a small fraction of `crates/*/src/`; the desktop
  alone has ~24 files (`commands.rs` is 11 682 lines, `snapshot.rs` 5 013).
- `app/CLAUDE.md` gives `DiffMode` as `Branch | Unstaged | Staged | History`. The enum
  (`app/state/mod.rs:100`) has **eight** variants **[corrected]** — the four above
  plus `Conflicts`, `Hidden`, `PrDiff`, `Tour`. The first pass said nine; its own
  list totalled eight.
- `agent_runtime.rs` (1339 lines) is absent from the module map and has **no callers
  in the workspace** — dead exported surface.

Scope caveat: this is a sample, not an audit. It covers the claims the optimisation work
touched, not the whole of each file.

## Deferred backlog

### Phase 0 — Measure before changing any default — **DONE**

See "Phase 0 results". Two of the three conclusions are now measured rather than
inferred. The remaining gap (two-limiter starvation, in-situ profiling) is listed
there.

### Phase 1 — Avoidable CPU (low risk, highest leverage)

| File | Change | Status |
|------|--------|--------|
| `er-engine/src/ai/prepared_diff.rs:34` | `ensure_diff_artifacts` builds and hashes the annotated diff on every call, then only *writes* it conditionally. Annotation is deterministic, so unchanged raw implies unchanged annotated: skip both the O(n) build and its SHA-256 when `diff-tmp` is already current and `diff-annotated` is present | **done** |
| `er-engine/src/app/state/mod.rs:4404` | Stat once into a `Vec<(idx, mtime)>`, then sort — removes `2n log n` syscalls | **done** |
| `er-engine/src/app/state/mod.rs:2913` | Gate the unconditional `reload_ai_state()` on the watch path — but on *sidecars changed OR `branch_diff_hash` moved*, not on the mtime check alone | **done** |
| `er-engine/src/app/state/mod.rs:4662` | Stop hashing every file on each watch event. **[corrected — the original instruction was wrong]** see below | **done** |
| `er-desktop/src/snapshot.rs:2782, :2897` | Replace the per-branch `merge-base --is-ancestor` loop with one `git branch --merged <base>` | **done** |
| `er-desktop/src/main.rs:951` | TTL the `git ls-remote` branch-base probe, matching its sibling loops | **done** — backs off while the tip is unchanged, resets on change |
| `er-engine/src/app/state/comments.rs` (`debug-agent.log` write) | Gate it behind `ER_DEBUG`; when enabled, append from the reader threads rather than buffering three copies | **done** — gated; the reader-thread append was not needed once the write stopped happening |
| `er-engine/src/highlight.rs:119` | Cache `SyntaxReference` per extension instead of re-resolving on every miss | **done** — the per-filename first-line sniff is memoised, which is the term that actually read the disk |
| `er-tui/src/main.rs:435` | Redraw only when state is dirty; correct the tick constants to the real 50 ms period | **tick constants done; redraw gate deferred** — see below |

Landed items carried three new tests in the first pass: the annotation skip
(counting annotation passes, not writes — a write count passes against the
unoptimised code), marker/content-file invariants including the pre-upgrade
marker, and the mtime sort from an unsorted list. The later items added more:
the per-file-hash set (including the eager-refresh regression), the lazy-hash
skip, the two merged-branch tests, the probe backoff, and the notification
trio.

#### Why the redraw gate is deferred

The tick-constant half is fixed: three timers were written against a 100 ms
period on a loop that polls at 50 ms, so the AI poll ran at 0.5 s instead of 1 s,
the watched-file rescan at 2.5 s instead of 5 s (doubling the `git check-ignore`
spawn rate), and notifications cleared after 1 s instead of 2.

The notification timer has since left the tick model entirely. Counting ticks
made the dwell stretch with loop load — the count advances once per iteration,
so a refresh in flight pushed "2 seconds" well past that — and `er-desktop`
never called `tick()` at all, so on that side the message was never cleared and
the frontend's text-based dedupe silently dropped every repeat. The engine now
stamps each message with a `seq` and leaves it set; the TUI clears it on a
wall-clock deadline, and the desktop raises one toast per `seq`. That also
removes the coupling this correction had introduced between an engine constant
and the TUI's poll rate.

The dirty gate is not implemented, deliberately. Skipping the draw on an idle
frame is only safe if *every* state mutation sets a dirty flag, and three of them
are invisible to the loop: `check_commands`, `poll_background_tasks` and
`drain_agent_log` all return `()`. A task retiring on the frame after its last
log entry would never be rendered — a stale frame in a TUI whose whole value is
live updates. Doing it properly means giving those three functions a change
report, which is a wide change whose riskiest part (missing a mutation site) is
the part a test cannot reach without a loop harness. That trade does not pay for
a #6-ranked idle-CPU win.

#### The watch-path item was wrong as written

The original instruction was "pass `compute_per_file_hashes = false` on the watch
path; gate `reload_ai_state()` on the existing `check_ai_files_changed` mtime
check". Both halves are wrong, and both fail silently rather than loudly.

**`compute_per_file_hashes = false` breaks review tracking.** `current_per_file_hashes`
is not only read by auto-unmark: all four mark-reviewed paths read it for the file
being marked, which by definition is not yet in `reviewed`. With the map empty they
would store an empty hash, and `auto_unmark_changed_reviewed` explicitly skips
entries whose stored hash is empty (`if stored_hash.is_empty() { return None; }`).
The result is that auto-unmark stops working permanently, with no error — review
markers would simply stop clearing when the underlying file changed.

The real waste is that the map is built for *every* file when only `reviewed` files
are consulted without a user action. Landed as lazy resolution: the watch path
caches hashes for `reviewed` paths only (`compute_per_file_hashes_for`), and the
five mark paths resolve a single path on demand through a new
`TabState::per_file_hash`, which falls back to the retained `raw_diff` and returns
the same empty sentinel callers already handled.

**The first version of this was wrong, and shipped with the same defect it was
supposed to remove.** It claimed the fallback worked because `raw_diff` "is kept in
both lazy and eager modes". It was not: `refresh_diff_impl` set `raw_diff = None`
in the eager branch, which is every diff at or under 200 KB — the common case. In
eager mode `per_file_hash` therefore missed the cache, found no raw diff, returned
the empty sentinel, and `auto_unmark_changed_reviewed` skipped it. That is verbatim
the silent, permanent auto-unmark failure this section rules out above. The
retention claim had been generalised from the constructors, which do keep it, and
never checked against the refresh path. Fixed by retaining `raw_diff` in the eager
branch too, which is bounded by the same 200 KB threshold that selected it.

That retention has a second effect worth naming: `raw_diff_for_review` served every
≤200 KB diff by shelling to git, and now answers from memory when the scope matches.
It is the same threshold and the same guard, so it reads as intended, but it is a
behaviour change and not only a bug fix.

Three existing tests asserted the invariant a stronger way than the behaviour needs
— that the map itself is fully populated — with the rationale "so newly-marked files
store a real hash". They now assert that behaviour through `per_file_hash`, which is
the accessor every mark path uses, and one of them also asserts the map is *empty*
when nothing is reviewed. **These three tests were deliberately changed, not
weakened to pass**: the behaviour they name is still asserted, and it was a test in
that set that caught the first draft, because two of them were passing only by
reading the cache and never exercising the fallback.

A fourth test now runs a real `refresh_diff` against a temp git repo, because all of
the above seed `raw_diff` by hand and so would green-light a state production never
reaches. That hand-seeding is exactly how the eager-mode defect survived a green
suite: the guards proved the fallback *works*, never that it is *live*.

The residual hazard is a future reader that touches `current_per_file_hashes`
directly instead of `per_file_hash`: it would silently get an empty hash for an
unreviewed file, which `auto_unmark_changed_reviewed` treats as "unknown" and skips.
`auto_unmark_changed_reviewed` is the one such reader left, and it is correct by
construction — it only ever looks up `reviewed` paths, which are exactly what the
cache holds. Any *new* direct reader would be wrong.

**An mtime-only gate breaks staleness.** `reload_ai_state` passes `branch_diff_hash`
into `load_ai_state`, and that is where each sidecar's recorded `diff_hash` is
compared to produce `is_stale`. The diff moving is therefore itself a reason to
reload: with an mtime-only gate, a diff-only change leaves old findings rendering as
current against a diff they no longer match. Landed with the gate reading
sidecars-changed **or** diff-moved, tracked by a new `last_ai_diff_hash` stamp set
alongside `last_ai_check` in `finish_ai_reload`.

#### What this says about the verification pass

The adversarial pass confirmed the *code structure* of this item — that
`refresh_diff_quick_with_unmark` really is `(false, true, true)` and that
`reload_ai_state` really is unconditional. It did not evaluate the *remediation*,
which is where both errors were. A verified claim is not a verified fix; the
Phase 1 table items should be re-read against the code before being implemented,
as this one was.

### Phase 2 — Runner correctness and throughput

**Add a per-agent timeout and make running reviews cancellable.** This is the
highest-value runner change: today one hung agent holds a slot forever and stalls an
entire arena round at the barrier join. Keep the `Child` on `BackgroundTaskHandle`
(mirroring `ArenaRunHandle`) so cancel can `kill()` it, and give every spawn a
wall-clock timeout. A timeout is what makes raising the cap safe.

**Close the ungated paths.** Route B (`spawn_agent_prompt`), C (card AI) and F
(`spawn_command`) through the same slot acquisition, and give the arena arbiter a
slot (`orchestrator.rs:999`). **[corrected]** the first pass listed three ungated
paths; there are four. Until then the cap is advisory.

**Re-derive `branch_diff_hash` on quick refreshes of a local-branch view.**
Pre-existing, not a regression from this branch — the skip is byte-identical at
the base commit. On the local-branch path, `refresh_diff_impl` only assigns
`branch_diff_hash` when `recompute_branch_hash` is true (`mod.rs:2910`), and
`reload_ai_state` *reads* that field rather than recomputing it
(`mod.rs:3366`/`:3369`). So a HEAD move the app did not perform itself — a commit
from a terminal — arrives on the watch path as a quick refresh
(`refresh_diff_quick_with_unmark`), leaves the hash where it was, and `is_stale`
is never re-derived: findings keep rendering as current against a diff they no
longer match. Committing from inside the app is unaffected, because
`submit_commit` calls the full `refresh_diff` (`comments.rs:1616`).

Note for whoever fixes it: removing the Phase 1 gate does not help. The reload it
skips would have been passed the same stale hash, so it computed the same wrong
answer — at the cost of re-reading every sidecar on every watch event. The fix is
to move the hash, not to stop skipping.

**Split the pool by workload.** Separate pools for background reviews and arena
rounds, each with its own configurable cap (e.g. `ai_hub.max_concurrent_arena_reviews`
alongside the existing key), sharing one hard ceiling on total processes. Removes the
starvation between workloads. Reconcile the desktop's `1..=6` option list with its
`1..=16` apply clamp — **[corrected]** this is one settings surface disagreeing with
itself, not a TUI/desktop mismatch.

**Fix the lying "Running" state.** Make `dispatch_pending_background_tasks` consult
real slot availability rather than `running_background_task_count()`, so it never
launches work it cannot run — and never parks a cancel-less thread on a job it may
wait indefinitely for.

**Shorten arena wall-clock.** Measured: rounds are serial and additive, reviewers
within a round are already concurrent, and the arbiter is a fixed serial cost. So
the lever is the round count. Either make the second round's cross-check cheaper
(feed it round 1's findings rather than a fresh full review) or make rounds
configurable with a cheaper default, and confirm the arbiter is the only thing that
must stay serial. Also emit each reviewer's result as it completes instead of after
the barrier join, so perceived latency tracks the slowest reviewer, not the batch.

**Stop spawning threads that park.** Drive reviewers through a bounded worker pool
consuming a queue, so thread count tracks the cap instead of the reviewer count.

**Log channel.** Make `AgentLogEntry.command_name` a shared `Arc<str>`; cap
`drain_agent_log` to a fixed number of entries per tick; bump a revision when entries
arrive so the desktop drains without waiting for the 30 s net; consider a bounded
channel with drop-oldest, since it is a log.

**Do not** batch reviewers into one process — arena's value is independent contexts
and models. Do not drop the CLI for the API: the prompts rely on the agent's own tool
access to the repo.

### Phase 3 — UI isolation

Extend `run_blocking` to the remaining heavy sync commands and widen the test at
`commands.rs:11493`. Priority: `arena_start` / `arena_start_batch`
(`arena_commands.rs:197`, `:241`, currently main-thread git + file IO),
`submit_github_review` (`commands.rs:3013`, git + `gh` under the lock),
`open_worktree` (`:5898`), then the `config_commands.rs` set.

Shrink the poll critical section: take a short lock for `drain_agent_log` /
`check_commands` / `poll_background_tasks`, release, then re-take for the snapshot
build. Move the sidecar parse in `reload_ai_state` off the lock where the caller
allows it.

Frontend: coalesce `overlaySerial` bumps to a rAF in `onHeightChange`
(`FlatDiffView.svelte:358`), and patch `baseGeometry` (`:372`) incrementally instead
of rebuilding every offset.

### Phase 4 — Re-evaluate

Only if Phases 1–3 miss the target: per-viewport `SNAPSHOT_DIFF_LINE_BUDGET`, a warm
long-lived agent process via the CLI's streaming mode, or deleting/wiring up the dead
`agent_runtime.rs` surface.

### Documentation corrections

**Status: 1, 2, 4, 5 and 6 done. 3 is half done.** They landed in the same
branch as Phase 1 rather than in a later pass, because most were the same
species of wrong claim the audit exists to find.

Also fixed while verifying the list: root `CLAUDE.md` still gave the mtime-sort
toggle as `Shift+R`, which this document corrects to `m` further up. That drift
is not one of the six items, so it had no line to be marked done against.

What remains from item 3: the stale strings that point users at the removed
repo-local config file — `er-tui/src/input/normal.rs:216` and the
`not_configured` texts at `app/state/mod.rs:6216` and `:6311` — plus the doc
comments at `app/state/mod.rs:4951` and `git/status.rs:936,946`, and four doc
pages (`docs/index.html`, `docs/config-reference.md:10,239`,
`docs/guide/configuration.html:24,183`). Those are a code change, and the item
asks for them to be approved separately, so they have not been touched.

1. Fix the `agent_slots` "before every agent subprocess spawn" claim in root
   `CLAUDE.md:82` and the "single hard cap" wording in `agent_slots.rs:5`, and state
   the six real spawn sites with their gate coverage.
2. Correct the poll interval (50 ms), the async-runtime statement (workspace-wide vs
   engine), the crate list (five members), and the frontend polling description
   (push-driven, 30 s fallback).
3. Remove the `.er-config.toml` description from `app/CLAUDE.md`, and fix the three
   stale runtime strings that point users at the removed file
   (`app/state/mod.rs:6102`, `:6197`, and the third site the verification pass
   located), plus the `docs/index.html` references. The string fixes are a code
   change; approve them separately.
4. Complete the `DiffMode` variant list in `app/CLAUDE.md` and root `CLAUDE.md`
   (eight variants, including `Tour`).
5. Note `agent_runtime.rs` as unwired, or delete it.
6. Correct the cap-range description: `1..=6` and `1..=16` are both the desktop's.

## Verification

- **Measurement is the gate.** `ER_AGENT_TIMING=1` and re-run
  `cargo test -p er-engine --test arena_timing -- --nocapture` after each phase;
  the harness asserts within-round concurrency and round additivity, so a
  scheduler change that serialises reviewers or overlaps rounds fails it.
- Watch-event cost: with `ER_DESKTOP_PROFILE_POLL=1 ER_LOG=profile`, a single save
  should stop triggering a SHA-256 pass and a full sidecar reload, while a genuine
  sidecar change still refreshes AI state — the mtime gate must not swallow updates.
  Cover this with a test that fails before and passes after.
- `ensure_diff_artifacts` skip: a second call with unchanged bytes must not build or
  hash the annotated diff. Assert on the O(n) work, not on the write (the write was
  already conditional, so a write-count test would pass before the fix).
- Timeouts and cancel: prove a hung agent is reclaimed (point a provider at a command
  that sleeps) and that cancelling a *running* review now terminates the process.
- Arena: confirm reviewers report as they finish rather than in one batch, and that
  a concurrent background review no longer starves the arbiter.
- Ungated paths: assert that N rapid "Ask AI" clicks cannot exceed the cap.
- `cargo test -p er-engine` (diff parser, slot pool, arena timing); `cargo build` for
  the workspace.
- Desktop UI ladder (`desktop-ui`): `svelte-check` → `bun test` → `vite build`.
  The tests are `bun:test`, not vitest.
- Jank: scroll a 100+ file diff and collapse/expand files while an agent runs.
- Riskiest regressions: the Phase 1 mtime gate and the Phase 2 scheduler change.
