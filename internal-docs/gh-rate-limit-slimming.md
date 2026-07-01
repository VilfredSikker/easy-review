# Spec: gh CLI rate-limit slimming (desktop background loops)

**Status:** proposed
**Branch target:** release branch (feature/perf change, not a `main` bugfix) — needs release notes
**Depends on:** nothing landed; findings 1–4 are independent and can ship incrementally
**Author:** (drafted from a trigger/frequency audit of all gh call sites, 2026-06-30)

---

## In plain terms

**The problem now.** The desktop app keeps three background timers running the whole time a PR
tab is open. Every 30 seconds it asks GitHub four separate questions at once (PR status, CI checks,
comments, reviews); every 30 seconds it asks "did the PR branch move?"; every 45 seconds it pulls
review comments. None of these timers check whether the answer they already have is still fresh —
they just re-ask on the clock, forever.

**Why it matters.** With **one** PR tab open that's roughly **760–920 GitHub calls per hour**, and it
multiplies for every extra open tab. GitHub doesn't just cap you at 5,000/hour — it has a stricter
"secondary" limit that punishes *bursts* and *concurrent* calls, and our 30s timer fires four calls
at the same instant. That's exactly what trips the rate limiter. The data to avoid most of these
calls is already sitting in memory — we compute a `last_updated` timestamp and then never look at it.

**The fix.** Make each timer check freshness before it calls out: "is what I have less than N seconds
old? then skip." Drop one call that's pure duplication (two timers both fetch CI status). And give all
gh calls a single front door (`run_gh`) so we can cache, de-duplicate, and back off in one place
instead of 40.

**TL;DR.** Stop re-asking GitHub the same question every 30 seconds when we already have a fresh
answer. Four small "skip if recent" gates cut ~900 calls/hour down to under ~50.

---

## Root cause (verified against source)

All rate-limit pressure comes from **three ungated background loops in `crates/er-desktop/src/main.rs`**.
The TUI poll/watch paths make **zero** gh calls; spawned AI agents are clean (arena has no gh
permission, review agents use `git diff` with `prepared_diff=true`).

Each loop is a bare `loop { …gh…; sleep(N) }` with no TTL / dirty-flag / freshness gate:

| Loop | Location | Cadence | Per-tick gh calls | ~calls/hr |
|---|---|---|---|---|
| gh-status fan-out | `main.rs:1034` | 30s | **4 concurrent** (`gh_pr_overview_remote_full` + `gh_pr_checks_remote` + `gh_pr_comments_overview` + `gh_pr_reviews`) | ~480 |
| comment sync | `main.rs:1106` | 45s | 2–4 (`gh_pr_comments_remote` + `gh_pr_review_threads_remote`; + `gh_pr_overview` for local-clone tabs) | ~240–320 |
| pr-head probe | `main.rs:950` | 30s | 1 (`gh pr view --json headRefOid`) | ~120 |

**Steady state ≈ 760–920 gh calls/hr per open PR tab.** The 30s loop's 4-way *concurrent* fan-out is
the worst for GitHub *secondary* rate limits (concurrency/bursts throttle well below the 5000/hr ceiling).

**Primary-evidence confirmation:**
- `main.rs:1034` — sleep is at the *end* of the body (line 1092), so the first iteration fires
  immediately on startup. The only gate is `gh_status_in_flight` (concurrent-overlap dedup), and the
  key is removed on completion (`main.rs:1087`), so every tick re-registers and re-fires `fetch_github_status`.
- `GithubStatusSnapshot.last_updated` is **written** (`commands.rs:626,651`) but its only other
  reference is a test fixture (`gh_status_cache.rs:170`) — it is **never read as a freshness gate**.
  The data for a TTL gate already exists; it's just unused.

---

## Ranked findings

### 1. background gh-status loop — 4-subprocess fan-out (~480 calls/hr) — BIGGEST WIN
- **Fires:** `main.rs:1034` loop; each tick → `fetch_github_status` → 4 parallel `gh` subprocesses via
  `thread::scope` at `commands.rs:559–578`.
- **Why eager:** `gh_status_in_flight` only blocks concurrent overlap; cleared on completion → every
  tick re-fires. `last_updated` never read. No TTL.
- **Feature relied on:** live PR status sidebar (CI checks, review decision, state, recent comments).
- **Fix:** before `fetch_github_status` (`main.rs:1066`), read `GithubStatusSnapshot.last_updated`;
  skip if `now − last_updated < 90s`. Move the sleep to the top of the loop to kill the startup burst.
  **Risk:** CI completing in <90s shows stale state until the next eligible fetch — acceptable (CI runs minutes).

### 2. background comment_sync loop (~240–320 calls/hr)
- **Fires:** `main.rs:1106` `loop { sleep(45s); … }` (80 ticks/hr). Each tick: `gh_pr_comments_remote`
  + `gh_pr_review_threads_remote`; plus `gh_pr_overview → gh_pr_checks_data` for local-clone tabs
  (`sync.rs:265–273`). No leading sleep — first iteration immediate.
- **Why eager:** `try_lock()` skips a tick only under mutex contention; no in-flight dedup, no TTL,
  no `last_synced` read-back. The CI-checks call here **duplicates** what the 30s loop already fetches.
- **Feature relied on:** auto-sync of teammates' PR review comments without pressing `G`.
- **Fix (two parts):** (a) thread-local `last_synced_oid`; skip if head OID unchanged AND <5 min since
  last sync — converts always-on → change-driven. (b) remove the `gh_pr_overview` CI call at
  `sync.rs:265–273` (the 30s loop owns CI freshness) → −~80 calls/hr of pure duplication.

### 3. pr_head_probe loop — `gh pr view --json headRefOid` (~120 calls/hr)
- **Fires:** `main.rs:950` `loop { sleep(30s); … }`. Bails when `tab.pr_number` is None; otherwise
  Phase 2 (`main.rs:988`) always spawns the gh subprocess.
- **Why eager:** `patch_pr_head_oid` (`pr_cache.rs:122–137`) compares OIDs **after** the subprocess
  returns — it controls only whether the snapshot revision bumps, not whether gh was called.
- **Feature relied on:** the "PR head updated on origin" stale pill (30s detection of a collaborator push).
- **Fix:** thread-local `HashMap<(slug, pr_number), (oid, Instant)>`; before Phase 2, if cached OID ==
  stored OID AND <60s elapsed, skip. Update only when `patch_pr_head_oid` returns true. Cheaper
  alternative: extend sleep 30s→120s (→30 calls/hr, still 5× faster than the 10-min cache sweep).

### 4. kick_active_gh_status — fires on every tab switch / open / close (~4 calls/switch)
- **Fires:** unconditionally from `select_tab` (`commands.rs:7086`), `close_tab` (`:7069`),
  `open_local_branch_impl` (`:4973`), `open_pr_review_impl` (`:5723`), `open_branch_for_project` (`:6273`),
  `set_active_project` (`:6629`). Each → full 4-subprocess fan-out.
- **Why eager:** `kick_active_gh_status` (`commands.rs:659–671`) never reads `last_updated`; if the
  background loop finished 5s ago, the in-flight set is empty and the switch fires a redundant 4-call burst.
- **Fix:** read `last_updated` first; return early if <10s old. Zero UX risk.

### 5. cold PR open — `gh pr diff` runs twice on cache miss (~1 extra call/cold open)
- **Fires:** first-ever PR open: `load_pr_open_inputs` (`commands.rs:5297–5595`) fetches once, then the
  cache-miss branch (`:5710`) calls `enter_pr_diff → refresh_diff → fetch_pr_diff_for_review → gh_pr_diff` again.
- **Fix:** guard `refresh_diff()` in `enter_pr_diff` (`state/mod.rs:2081`) with `if self.files.is_empty()`
  — files are already populated from the pre-loaded raw_diff.

### 6. open_remote_pr — `gh_pr_overview_remote` fetched twice (~1 extra call/remote open)
- `do_open_remote_pr` (`commands.rs:4490–4516`) calls `gh_pr_overview_remote`, then `kick_active_gh_status`
  fires `gh_pr_overview_remote_full` (same endpoint, richer fields).
- **Fix:** use `gh_pr_overview_remote_full` once, pre-seed it into the status cache; the Finding-4 TTL
  gate then suppresses the redundant background fetch.

---

## Cross-call batching opportunities (synthesis over the full inventory)

| Context | Current | Merge into |
|---|---|---|
| `new_remote()` open burst | `gh_pr_metadata_remote` (baseRefName, headRefName) + `gh_pr_overview_remote` (9 fields incl. same 2) | Drop `gh_pr_metadata_remote`; read from overview (strict subset) |
| `fetch_github_status` fan-out | overview_full + reviews + comments_overview (3 separate `gh pr view`) | One `gh pr view --json <union of 14 fields>`; keep `gh_pr_checks_remote` separate (`gh pr checks`) → **4 subprocesses → 2** |
| `open_pr_review` cold open | `gh_pr_for_current_branch` + `gh_pr_commits` + `gh_pr_overview` | One `gh pr view --json number,baseRefName,commits,title,body,state,author,url,headRefName,reviews` |
| `refetch_and_refresh_diff` (lazy tab restore) | `gh_pr_base_branch` + `gh_pr_commits` (sequential) | `gh pr view --json baseRefName,commits` (extends `gh_pr_branch_names` pattern) |
| headRefOid pre-steps (push_comment, submit_review + 3 remote variants) | 1 `gh pr view --json headRefOid` per submit | Read from `pr_cache` (already tracked by probe loop); fetch only on miss |

Precedent already in the codebase: `gh_pr_branch_names` collapses two lookups into one `gh pr view`.

---

## Structural recommendation — `run_gh` chokepoint

~40 raw `Command::new("gh")` sites in `github.rs`, no central wrapper → no place for caching, dedup,
backoff, or instrumentation without touching all 40. Propose `run_gh(argv, cwd) -> Result<String>` as
the single path:

- **Short-TTL response cache** keyed by `(argv_canonical, cwd)` — cache read calls 30–90s; bypass
  writes (`-X POST/PATCH/DELETE`, `gh pr edit`, `gh pr review --approve`); evict by key on any write to
  the same PR.
- **In-flight dedup** — second caller of an in-flight argv waits and gets the first result (kills the
  4-way concurrent fan-out hitting the same endpoint).
- **Exponential backoff on rate-limit** — detect stderr `API rate limit exceeded` / `secondary rate
  limit`; back off 60s, doubling, capped 10 min; surface a `RateLimitError` for a UI indicator.
- **Call counter behind `ER_DEBUG=1`** — one log line per call (argv, hit/miss, elapsed) to `/tmp/er_debug.log`.

Per-loop TTL fixes (1–3) remain complementary — they stop calls reaching `run_gh`; the cache is the
backstop. Migrate one function at a time, starting with the 4 fan-out callers and the 5 headRefOid pre-steps.

**Scope limit (important): `run_gh` cannot throttle agent-spawned gh.** It wraps `gh` calls made by
*the Rust process*. Spawned review/arena agents that are permitted `Bash(gh ...)` shell out to `gh` as
their own subprocesses, entirely outside any Rust wrapper — `run_gh`'s cache / dedup / backoff never
see them. Today this is moot because every desktop review runs `prepared_diff=true`, whose allowlist has
no `gh` (so agents are *denied* gh in headless mode — see "Ruled out"). The load-bearing invariant is
therefore the allowlist, not `run_gh`: if a future feature ever grants an agent `Bash(gh ...)`, it needs
its *own* call cap (a wrapper script or a hard per-run limit) — the background-loop gates and `run_gh`
will not cover it.

---

## Ruled out (verified gated / not contributors)

- `run_gh_pr_diff_for_open` — `pr_open_cache` LRU (32) + in-flight dedup. ~0/hr idle.
- `new_remote` (`gh_pr_metadata_remote`/`diff_remote`/`commits_remote`) — tab-existence dedup; once per
  unique remote PR per session. The old 300s auto-refresh loop was explicitly removed (`main.rs:845–853`).
- `prefetch_pr_open` — 150ms frontend debounce + cache hit + in-flight dedup.
- **Spawned AI agents** — arena agents have **no** gh permission; review agents use `git diff`
  (`prepared_diff=true`) on all desktop paths. (The `agent_runtime.rs:168` `Bash(gh pr *)` allowlist is
  not exercised by the desktop review paths.) Verified at the permission layer: desktop review/expert/
  triage/professor/validate spawns all use `build_*_prepared_diff` prompts whose `--allowedTools` list
  (`comments.rs`, `prepared_diff` branch) has **no** `Bash(gh …)`; in headless `--print` mode a tool
  outside the allowlist is *denied*, so a prepared review cannot call gh even if prompted to. Arena
  access profiles are both `PreparedArtifacts` (no gh). The theoretical ceiling, if a gh-capable profile
  (`RemoteArtifacts`, or the non-prepared review branch) were ever wired to a desktop review, is
  *unbounded gh-per-agent* (the allowlist is a gate, not a quota) × the agent-slot cap
  (`max_concurrent_reviews`, default 3, max 16) — and, per the `run_gh` scope note above, no Rust-side
  mechanism would throttle it. The invariant that keeps reviews at **0** gh calls is "reviews stay
  `prepared_diff` / gh stays off the agent allowlist."
- **TUI event loop / watcher / input handlers** — **zero** gh calls in any poll/tick path; all TUI gh
  calls are user-key-driven.
- `ensure_gh_installed` / `gh auth status` — startup once-shot.

---

## Suggested order of work (cheapest-highest-impact first)

1. **TTL gate on the 30s gh-status loop** (`main.rs:1034–1092`, read `last_updated`) — **−~400/hr.**
2. **TTL gate on `kick_active_gh_status`** (`commands.rs:659–671`, 10s) — <10 lines; depends on #1. **−4/switch.**
3. **Gate comment-sync loop on head-OID + drop CI duplication** (`main.rs:1106`, `sync.rs:265–273`) — **−~230/hr.**
4. **Thread-local OID cache in pr_head_probe loop** (`main.rs:950`) — **−~120/hr on idle PRs.**
5. **Collapse fan-out 4→2** (merge overview+reviews+comments into one `gh pr view --json`) — halves remaining subprocesses.
6. **Fix cold-PR double diff-fetch** (`state/mod.rs:2081`) — low risk, −1/cold open.
7. **Cache headRefOid for comment-push pre-steps** — −1/submit.
8. **Introduce `run_gh` chokepoint** — structural backstop after per-loop fixes validate.

**Fixes 1–4 are the rate-limit fix; 5–8 are reinforcing.**

---

## Audit method

28-agent workflow keyed on **trigger/frequency** (the call site is never the finding — the trigger is).
Phase 1: inventory all 41 gh wrappers + map 5 trigger-domains (desktop poll, desktop open/tab, TUI loop,
sync loops, spawned agents) with gating + frequency. Phase 2: each eager candidate verified
adversarially — the verifier's job was to *disconfirm* by finding an existing gate; **0 of 13 confirmed
candidates turned out gated.** Phase 3: synthesis over the full table for batching + structural fixes.
Highest-impact claims re-verified against source by primary read.
