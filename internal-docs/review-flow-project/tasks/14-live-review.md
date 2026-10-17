# 14 — Review while the agent writes

Status: needs-design
Idea: 19 in the original idea list (summarised in ../README.md)
Depends on: 07-default-review-flow.md (the Quick depth), 08-review-convergence.md (rounds, no-repeat ledger, changes-only runs)
Impact: H · Complexity: H · Size: L
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.
>
> **This task is `needs-design`, and it touches a decision already made.** `../README.md` says nothing runs on its own: "Re-runs after new commits are offered to the person, not started automatically." A live review starts on its own by nature. Step 1 writes a design that keeps it opt-in and asks the person to approve that exception. Write no code before that approval.

## In plain terms
- **The problem** — er watches an agent's changes land live, but AI review only happens when a person clicks. By then the agent has finished, a PR may exist, and every routine mistake is waiting to be found at once.
- **Why it matters** — The blog's team gets most changes "reviewed and fixed by Claude at least once before a human is asked to look". Catching routine mistakes while the agent is still working means the PR-time review has less to find and the person reads less.
- **The fix** — An opt-in switch. When it is on, er waits until a file in the local branch view stops changing, then runs a cheap Quick review on the files that have settled. Its findings show up as "found while writing". When the PR-time review runs later, it knows what the live pass already raised and does not raise it again.
- **TL;DR** — An optional, cheap review of files once the agent stops touching them, whose findings carry into the real review instead of being found twice.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> Now, review runs continuously, more like spellcheck while you write. Most changes have been reviewed and fixed by Claude at least once before a human is asked to look. This catches most routine mistakes, and saves team attention for higher-level questions and scrutiny.

From the same post, on cost:

> Noise costs more when the author is an agent. A person can wave off a nit in seconds. An agent fixes it, which means another commit and another round of review.

## Mission & definition of done
Design, get approval for, and then build an opt-in live review for the local branch view. It must have a clear definition of "settled", a hard cost bound, and a way for its findings to reach the PR-time review without being found twice.

- [ ] A design note and ADR under `docs/adr/` answer the open questions in Step 1. The person has approved them, including the exception to "nothing runs on its own".
- [ ] Live review is off by default. A person turns it on with one global setting (ADR 0005: config is global-only), shown in both front ends.
- [ ] With it on, a file counts as settled only by the approved rule. No live run starts while any file in the batch is still changing.
- [ ] At most one live run per tab at a time. It takes a `Workload::Background` agent slot (ADR 0021). It never starts while a review the person started is running or queued.
- [ ] Live findings are stored apart from `review.json` and shown apart, labelled "found while writing".
- [ ] Each live finding's no-repeat key (task 08) enters the round ledger. A later PR-time review does not raise it again; it carries it or closes it.
- [ ] Live runs never count toward task 08's round cap.
- [ ] Remote tabs, the PR diff view, and the Unstaged/Staged/History modes never start a live run.

**Risk:** high. This is the first thing in er that spends agent runs without a click, on a machine that is already running a writing agent.

## Before you start (orientation)
- **Branch:** see `../README.md` "Branching". This is a feature: use the current release branch.
- **Commands:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine <filter>`
  - `~/.claude/scripts/locked.sh . cargo test -p er-tui <filter>`
  - `~/.claude/scripts/locked.sh . cargo test -p er-desktop <filter>`
  - `~/.claude/scripts/locked.sh . just test-ui`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`, `just docs-check`, `just comments-check`
- **The map** (confirmed at c96dc0f12):
  - `crates/er-engine/src/watch/mod.rs` — `FileWatcher::new(root: &Path, debounce_ms: u64, tx: mpsc::Sender<WatchEvent>)`; `WatchEvent::FilesChanged(Vec<String>)` carries absolute paths. It drops `/.er/` paths and all of `.git/` except `index` and `refs/`. Read `crates/er-engine/src/watch/CLAUDE.md`.
  - `crates/er-tui/src/main.rs:443` — `FileWatcher::new(root, 500, watch_tx.clone())`. The event drain at line 497 keeps the paths, then calls `refresh_diff_quick_with_unmark()` after a 200ms coalescing window.
  - `crates/er-desktop/src/main.rs:730` — `FileWatcher::new(Path::new(root_path), 250, tx.clone())`. Lines 767–769 match `FilesChanged(_)` and **discard the paths**. The loop waits with `rx.recv_timeout(poll_interval)` (400ms) and `continue`s when no event arrived. It watches only the target from `desired_local_branch_watch` (line 2134).
  - `crates/er-engine/src/app/state/mod.rs:2908` — `pub fn refresh_diff_quick_with_unmark(&mut self) -> Result<()>`. Both front ends call it on watch events, so it is the single engine seam.
  - `crates/er-engine/src/app/state/mod.rs:734` — `pub reviewed: HashMap<String, String>` (path → per-file diff hash at mark time) and `reviewed_file_hashes` (line 743). `refresh_per_file_hashes_and_unmark` (line 5219) hashes only the wanted paths with `ai::compute_per_file_hashes_for`. That is the pattern to copy for settle tracking.
  - `crates/er-engine/src/ai/loader.rs:70` — `pub fn compute_per_file_hashes_for(raw_diff: &str, wanted: &HashSet<String>) -> HashMap<String, String>`.
  - `crates/er-engine/src/agent_slots.rs` — `pub enum Workload { Background, Arena }`; `acquire` (line 202); the doc comment at the top explains why the caps exist.
  - `crates/er-engine/src/app/state/comments.rs` — `spawn_background_agent_task` (line 3045, refuses a second task of the same kind and target, queues behind the slot cap); `spawn_background_review` (line 2808).
  - `crates/er-desktop/src/auto_triage.rs` — header: "Auto-dispatch on PR cache refresh is disabled". The `auto_triage` project flag is kept and queues nothing. Commit `5bf7d4e3b` ("improvements to build, add triage runs", 2026-06-09) turned it off and gives no reason.
- **Conventions to follow** — hash only the paths you need (`crates/er-engine/src/app/state/mod.rs:5219`):

  ```rust
  fn refresh_per_file_hashes_and_unmark(&mut self, raw: &str, auto_unmark: bool) {
      let wanted: HashSet<String> = self.reviewed.keys().cloned().collect();
      self.reviewed_file_hashes = ai::compute_per_file_hashes_for(raw, &wanted);
      if auto_unmark {
          self.pending_unmark_count = self.auto_unmark_changed_reviewed();
      }
  }
  ```

- **Mental model** — The watcher reports that paths changed; it never decides when to refresh (watch/CLAUDE.md). Each front end debounces, then calls `refresh_diff_quick_with_unmark`, which recomputes the diff and per-file hashes for reviewed files. A file "settles" when nothing reports a change for a while, so it is detected by the *absence* of events. Something has to check on a timer, and the TUI loop and the desktop watcher thread both already wake up regularly. Agent runs go through `spawn_background_agent_task`, which enforces one task per kind and target and the slot cap. Task 08 gives each view bucket a round history and a ledger of no-repeat keys. Task 07 gives a Quick depth: the general review (`build_review_prompt_prepared_diff`, run through 07 Step 5's background-queue path), then a seeded arbiter pass over its findings (07 Step 6). That flow writes `review.json`, which a live run must never touch, so live review reuses 07's finder prompt and runs it on its own path.
- **ADRs to read first:**
  - 0004 — per-view buckets and their two exceptions. Carrying live findings from the branch bucket into the PR bucket would be a third.
  - 0005 — config is global-only; the switch is a global key.
  - 0011 — the desktop pushes revisions; a live-run result must bump the revision.
  - 0021 — slot cap; a new spawn path is uncapped unless it acquires.
  - 0025 — tour reuse across buckets on a matching hash, the precedent for a cross-bucket read.
  - Task 08's ADR (written in that task's Step 1) — the no-repeat key and the ledger.

## Scope boundaries
- **In scope:** the design and ADR; the global switch; settle tracking in the engine; the live-run trigger with its cost limits; a live sidecar and its display; feeding live keys into task 08's ledger; the PR-time review reading them.
- **OUT of scope / DO NOT TOUCH:**
  - Starting anything on PR refresh, push, or GitHub events. Only local file settling is in scope.
  - Re-enabling `auto_triage` dispatch.
  - Remote tabs (no watcher) and the PR diff view.
  - Changing the watcher's debounce values or `watch/mod.rs` filters.
  - Fixing findings automatically. Live review only reports.
  - Any change to task 07's depths or task 08's cap rules beyond "live rounds do not count".

## Steps

### Step 1 — Design note and ADR (stop for approval)
- **Goal:** Answers to the open questions, approved by the person, before any code.
- **Why / invariant:** It breaks a recorded decision and spends money with no click. Only the person can approve that.
- **Do this first:** run `git show 5bf7d4e3b -- crates/er-desktop/src/auto_triage.rs crates/er-desktop/src/main.rs` and read why auto-dispatch was turned off. If the diff shows a reason (cost, noise, a loop), the design must say how live review avoids it. If no reason is visible, say so in the note.
- **Files:** new `docs/adr/NNNN-live-review-on-settled-files.md` (next free number when you start; tasks 05–08 also add ADRs). Read `docs/agents/writing-docs.md` first.
- **Answer each question. Options and the recommendation follow.**
  1. **What "settled" means.**
     - (a) Quiet period per path: no watcher event for that path for N seconds. Needs watcher paths, which the desktop throws away (`main.rs:767-769`), and absolute paths must be mapped to diff paths. Editor swap files and formatters cause noise.
     - (b) Per-file diff hash unchanged for N seconds. Checked in the engine at each refresh and on a timer, hashing only candidate paths with `compute_per_file_hashes_for`. It needs no watcher paths, works the same in both front ends, and ignores saves that do not change the diff.
     - (c) Commit boundary: review the files a new commit touched. `.git/refs/` events already pass the watcher filter. It is cheap and coarse, and misses agents that do not commit.

     **Recommend (b)**, with N = 90 seconds by default, plus a tab-wide quiet rule: no refresh for 20 seconds before a run starts, so a batch is not sent while the agent is mid-burst. Offer (c) as a later setting.
  2. **Cost bound.**
     - Quick depth only, meaning 07's finder prompt (`build_review_prompt_prepared_diff`) and nothing heavier. At Quick, 07 also runs a seeded arbiter pass, which is a second agent call. State whether live runs include it. Recommend: no, so a batch costs one run; the PR-time review verifies later.
     - At most one live run per tab at once (the existing kind-plus-target check in `spawn_background_agent_task` gives this when the kind is `"live-review"`).
     - Charged to `Workload::Background`.
     - Never starts while any task the person started is running or queued for that tab.
     - At most M live runs per hour per tab (recommend M = 6).
     - A batch takes every settled file that changed since the last live run, so a burst of ten files costs one run.

     State the worst case in agent runs per hour.
  3. **Opt-in.** A global key `ai_hub.live_review` (bool, default false), listed in `crates/er-engine/src/config/settings.rs` beside `ai_hub.max_concurrent_reviews` (about line 184). There is no per-repo config (ADR 0005). Show it in the desktop settings and as a TUI toggle.
  4. **Where live findings live.** Recommend a host-read, agent-written `live.json` in the branch bucket, with the same schema as `review.json`, written through the changes-only path from task 08 but into its own file. It is shown as its own group ("found while writing"). It never writes `review.json`, so the person's own review is never overwritten by a run they did not start. Live runs are recorded in `rounds.json` with a kind `live` that the cap ignores.
  5. **Carrying into the PR-time review.** Live keys are written to task 08's ledger in the branch bucket as `Raised` with origin `live`. The PR-time review runs in the PR bucket. Recommend a read across buckets, as tour reuse does (ADR 0025): when the PR-time review plans its first round, it reads the branch bucket's ledger and live findings for the same branch, passes the open ones in as prior findings (task 08's changes-only appendix), and treats their keys as already raised. The ADR must record this as a third exception to ADR 0004, and that it is a read only, with no copy.
  6. **What the person sees.**
     - A small "live" indicator while a run is going.
     - The "found while writing" group.
     - In the TUI, the status line says when a live run starts and ends, using the existing `notify` dwell.
     - Nothing pops up or takes focus.
- **Verify:** `just docs-check` → exit 0.
- **Then STOP** and ask the person to approve the ADR, the exception to "nothing runs on its own", and the defaults (N, quiet window, M). Record the answers under `## Comments` in this file.

### Step 2 — Settle tracker in the engine (tests first)
- **Goal:** `TabState` knows which diff files have settled since the last live run.
- **Why / invariant:** Pure state with an injected clock, so it is testable without a watcher or a real agent.
- **Files:** new `crates/er-engine/src/app/state/live_review.rs` (declare it in `crates/er-engine/src/app/state/mod.rs` beside `pub mod background;`, line 2). Add a field `live: LiveReviewState` on `TabState`, initialised in every constructor where `reviewed_file_hashes: HashMap::new()` appears (lines 1586, 1721, 1850, 1992 at c96dc0f12).
- **Do this:**
  - `LiveReviewState { seen: HashMap<String, (String, Instant)>, last_refresh: Option<Instant>, last_run_hashes: HashMap<String, String>, runs_this_hour: VecDeque<Instant> }`.
  - `fn observe(&mut self, current: &HashMap<String, String>, now: Instant)`: for each path, if the hash differs from `seen`, reset its timestamp; drop paths no longer in the diff; set `last_refresh = Some(now)`.
  - `fn settled_batch(&self, now: Instant, settle: Duration, quiet: Duration) -> Vec<String>`: paths whose hash has been stable for at least `settle` and differs from `last_run_hashes`, but only if `now - last_refresh >= quiet`. Sort the result.
  - Call `observe` from `refresh_diff_impl` (line 3184) on the quick-with-unmark path, only when `self.config.ai_hub.live_review` is on and the mode is the local branch view. `TODO(executor): confirm how the tab reads config — search for "ai_hub" in crates/er-engine/src/app/state/mod.rs`. Hash all diff paths with `compute_per_file_hashes`, and only when the switch is on.
- **Tests** (inline in `live_review.rs`): `file_settles_after_window`, `hash_change_resets_settle_timer`, `no_batch_while_tab_not_quiet`, `file_already_run_at_same_hash_is_not_rebatched`, `removed_file_leaves_seen`.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine live_review` → all pass. Run once with `todo!()` bodies first → they fail.

### Step 3 — Trigger, guards and spawn
- **Goal:** One engine method decides whether to start a live run now, and starts it.
- **Why / invariant:** Every guard from Step 1, item 2 sits in one place, and the spawn takes a slot (ADR 0021).
- **Files:** `crates/er-engine/src/app/state/live_review.rs`; spawn through `spawn_background_agent_task` (`comments.rs:3045`) with kind `"live-review"`.
- **Do this:** `pub fn live_review_tick(&mut self, now: Instant)` on `App`. It returns early when:
  - the switch is off;
  - the tab is remote or not in the local branch view;
  - any background task for this target is running or queued (inspect `self.background_tasks` and `self.pending_background_tasks`, as `spawn_background_agent_task` does at its top);
  - the hourly cap is reached;
  - `settled_batch` is empty.

  Otherwise it builds the prompt and spawns. Task 07 has no separate Quick prompt builder: its Quick depth spawns `build_review_prompt_prepared_diff` (`crates/er-engine/src/ai/prompts.rs:451`) through the background queue (07 Steps 5–6). Use that builder with task 08's `changes_only_appendix` (08 Step 4) over the batch, and change the output target from `review.json` to `live.json` (Step 1, item 4). If 08's appendix is missing, use `file_scope_appendix` (`prompts.rs:824`) instead. Do not call 07's `start_review_flow`: it writes `review.json` and runs triage. Then spawn, record `last_run_hashes` and the run time, and `notify("Live review started (N files)")`.
- **Tests:** `tick_does_nothing_when_switch_off`, `tick_skips_while_person_task_running`, `tick_respects_hourly_cap`. Inspect the queued task instead of running an agent. Use the seam 07 Step 5's Verify uses: an App helper copied from `test_app` (`crates/er-engine/src/app/state/comments.rs:4028`) with `config.agent.command = "true"`, so spawns exit at once, and the `serial()` guard (`comments.rs:4024`), because the slot pool is process-wide. Read the queued task from `pending_background_tasks` / `background_tasks`.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine live_review` → pass.

### Step 4 — Call the tick from both front ends
- **Goal:** Settling is detected without needing a watcher event.
- **Files:**
  - TUI: `crates/er-tui/src/main.rs`, in the main loop near `app.poll_background_tasks()` (line 537 at c96dc0f12): `app.live_review_tick(Instant::now());`.
  - Desktop: `crates/er-desktop/src/main.rs`, in the watcher thread loop **before** `if !got_event { continue; }` (about line 774). Lock `watcher_app`, call `live_review_tick`, and if a run started, bump the revision with `profile_log::bump_desktop_revision(&watcher_desktop_rev, "live_review")`, as line 760 does (ADR 0011).
- **Watch out for:** the desktop loop holds no lock between iterations. Take the lock briefly and never across the spawn's IO.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-tui` and `~/.claude/scripts/locked.sh . cargo test -p er-desktop` → pass.

### Step 5 — Live sidecar, ledger feed and display
- **Files:** `crates/er-engine/src/ai/loader.rs` (load `live.json` into a new `AiState.live` field; do not merge it into `review`); task 08's `rounds.rs` (record a round of kind `Live` that the cap ignores, and mark keys `Raised` with origin `live`); TUI `crates/er-tui/src/ui/panel.rs` (a "found while writing" group near `render_file_findings`, line 410); desktop snapshot `crates/er-desktop/src/snapshot.rs` (new explicit field; ADR 0006) and a group in the findings list of `desktop-ui/src/lib/components/AiReviewCard.svelte` (the `findings-list`, `{#each ordered as finding}` near line 485 at c96dc0f12). Tasks 06 and 10 already group that list (Pre-existing, Security) through a shared row `{#snippet}`; add "found while writing" as one more group with the same snippet. In both front ends, place it after the change's own findings and before 06's Pre-existing group, with 10's Security group first.
- **Types to add to task 08's module:** 08 Step 2 defines `RoundKind { Full, ChangesOnly, FullByHand }` and `LedgerEntry { state, pattern, round }` with no origin. Add a `Live` variant to `RoundKind` and `#[serde(default)] pub origin: String` to `LedgerEntry` (empty for 08's own entries, `"live"` here). 08 Step 2 writes the cap as a count of `Full`, `ChangesOnly` and `FullByHand` rounds, so `Live` is ignored without changing 08's rule; the test `live_round_does_not_count_toward_cap` proves it.
- **Tests:** `live_round_does_not_count_toward_cap` (in `rounds.rs`); `live_json_loads_apart_from_review` (in `loader.rs` tests).
- **Verify:** engine, TUI, desktop and UI tests pass; `just lint-ui` → exit 0.

### Step 6 — PR-time review reads live findings
- **Goal:** The first PR-time round treats open live findings as prior findings.
- **Files:** task 08's `plan_next_round` (in `rounds.rs`) gains a cross-bucket read of the branch bucket for the same branch. Resolve that bucket through `storage.rs`; never build the path by hand (ADR 0003). `TODO(executor): confirm the branch-bucket resolver — search for "view-buckets" or "fn branch_bucket" in crates/er-engine/src/storage.rs`.
- **Tests:** `pr_round_one_treats_live_keys_as_raised`.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine rounds` → pass.

## Gotchas & landmines
- The desktop watcher discards changed paths. Option (b) avoids needing them; do not widen `WatchEvent` unless the approved design picks (a).
- `refresh_diff_quick_with_unmark` runs on every watch event. Hashing every file there costs time on large diffs. Only do it when the switch is on.
- The writing agent and the live reviewer share the worktree. The reviewer must read only the prepared diff and write only into the bucket; the existing prepared-diff prompts already say so.
- `spawn_background_agent_task` refuses a second task of the same kind and target, so a stuck live run blocks the next one. That is acceptable; show its status.
- `just comments-check` rejects comments citing task numbers.
- Watch paths are absolute; diff paths are repo-relative. Only option (a) has to map them.

## Decision log (forks pre-answered)
- **If task 07's Quick depth does not exist:** use the general prepared-diff prompt with the file-scope appendix (Step 3), and note it under `## Comments`.
- **Live runs and 07's flow (consistency pass, 2026-10-17):** an earlier draft of this file said 07's Quick was "one finder, then Validate". 07 recommends the seeded arbiter instead, and its flow writes `review.json`. So a live run reuses only 07's finder prompt, writes `live.json`, and never starts the flow. Whether a live run also gets a verify pass is a Step 1 question (recommend: no).
- **If task 08 is not built:** stop. The ledger and changes-only runs are what make the carry-over work.
- **If the hourly cap is reached:** skip without notifying each time. Show the cap once per hour in the status line.
- **If a live run fails:** log it in the agent log and do not retry until the batch's hashes change again.
- **If the person starts a review while a live run is going:** let the live run finish (it holds a slot); the person's run queues behind it, as today (ADR 0021). Do not cancel either.

## Full verification
1. `~/.claude/scripts/locked.sh . cargo test -p er-engine live_review` → pass.
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine` → pass.
3. `~/.claude/scripts/locked.sh . cargo test -p er-tui` → pass.
4. `~/.claude/scripts/locked.sh . cargo test -p er-desktop` → pass.
5. `~/.claude/scripts/locked.sh . just test-ui` → pass.
6. `~/.claude/scripts/locked.sh . just clippy` and `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
7. `just docs-check` and `just comments-check` → exit 0.
8. By hand (`just run` in a scratch repo, switch on):
   - Edit a file, wait past the settle window → one live run, and its findings appear under "found while writing".
   - Keep editing → no run.
   - Start a review by hand while a live run is going → it queues.
   - Open the PR view and review → a live finding is carried forward and raised only once.
   - Switch off → no runs.

## Rollback
Turn the switch off (default off), or revert the commits. `live.json` files left in buckets are not read by older builds.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- After Step 1, always: the person must approve the exception to "nothing runs on its own" and the cost defaults.
- `git show 5bf7d4e3b` reveals a reason for disabling auto-dispatch that live review would repeat.
- The approved design needs watcher paths (option a), which changes the desktop watcher thread.
- Task 08's ledger or `plan_next_round` does not exist or has a different shape.

## Comments
