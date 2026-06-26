# Spec: Background prefetch + "New diff ready" (deferred-apply staleness)

**Status:** proposed
**Branch target:** release/v0.4.0 (or a follow-up)
**Depends on:** the staleness fix + `pr_open_cache` persistence already landed this cycle
**Author:** (drafted from desktop staleness work, 2026-06-24)

---

## In plain terms

**The problem now.** When a PR you have open gets new commits on origin, a pill appears:
"PR head updated on origin — Sync to refresh." Clicking Sync does the slow part *then*:
it fetches the new head, re-diffs, re-parses, and swaps the result in. You wait while it works.

**Why it matters.** The slow work (git fetch + diff + parse) happens *on the click*, in the
foreground. For a large PR that's a visible, blocking wait every time origin moves.

**The fix.** Split detection → fetch → render into two clicks' worth of timing:
1. We *detect* staleness (already cheap and automatic).
2. We *fetch + parse* the new diff **in the background**, into a staging buffer.
3. Once it's ready, the pill flips from **"New commits on origin"** to **"New diff ready —
   click to view."**
4. The click now just **swaps in the already-parsed buffer** — instant, no fetch.

The reviewer still explicitly opts in to seeing the new diff (the diff never shifts under them
mid-review), but the wait is gone.

**TL;DR.** Do the slow work before the click, not during it. The button stops meaning "go fetch"
and starts meaning "show me what you already prepared."

---

## Honest scoping of the benefit (read before building)

This reduces **blocking / visible** load to near-zero — clicks become instant. It does **not**
reduce *total* git/gh work; the same diff is fetched+parsed, just earlier. Naïve prefetch can
*increase* total work if it prefetches diffs nobody views.

**The discipline that makes it pay off: prefetch only the ACTIVE (visible) tab, debounced.**
Never background-fetch every stale tab. If the real goal were *fewer total network calls*, the
lever would be different (longer cache TTL / fewer polls) — not prefetch. This spec optimizes for
**never-block, predictable UX**, which is the stated goal.

---

## Current architecture (what already exists)

| Concern | Where | Behavior |
|---|---|---|
| Staleness detection | `crates/er-desktop/src/snapshot.rs` `diff_stale` (~1856) | Compares pr_cache `head_oid` (refreshed every 10 min, `main.rs:1248`) vs `tab.last_diff_head_oid`. Pure in-memory compare. Cheap. |
| The pill (one state) | `DiffStaleSnapshot` (snapshot.rs) → `BranchContextBar.svelte` | Single message + a Sync button. |
| Sync action | `force_refresh_diff` (`commands.rs:2328`) → `refetch_and_refresh_diff` (`mod.rs:2060`) | **Fetch + apply are fused:** force-fetches `refs/er/pr/<n>/head`, re-diffs, re-parses, refreshes `pr_commits`, mutates the live tab (`&mut self`), then aligns `last_diff_head_oid` to clear the pill. |
| Diff cache / buffer | `pr_open_cache` (`commands.rs:112`), persisted (`pr_open_cache.rs`) | `HashMap<PrOpenCacheKey, PrOpenCacheEntry{raw_diff, pr_data, pr_commits}>`, **freshness-gated by head_oid**, 32-entry cap, 2 MB/entry persist guard. Read via `cached_pr_open_entry` (`commands.rs:4991`). |
| Off-thread infra | `run_blocking`, worker `std::thread::spawn`, `desktop_revision` bump → poll picks up | e.g. `kick_deferred_tab_refresh` (`commands.rs:6870`), gated by `needs_initial_refresh`. |

**Key realization:** `pr_open_cache` — freshness-keyed by head_oid and already persisted — **is the
staging buffer.** A background fetch at the new head naturally lands as a fresh cache entry, and
"apply" becomes a cache hit. Most of the buffer already exists.

---

## The core engineering challenge

`refetch_and_refresh_diff` **fuses fetch and apply** — it parses the new diff *and* swaps it into
the live tab in one `&mut self` call. For background prefetch we need the fetch **without** the
swap, so the visible tab is untouched until the user clicks.

So the central task is a **seam split**: factor a non-mutating
`fetch_pr_diff_to_buffer(pr_number, head_oid) -> Result<StagedDiff>` that fetches + parses + returns
(and writes to `pr_open_cache`) without touching the live `TabState`. The open path
(`open_pr_review_impl`) already does "fetch diff → raw string → cache → build tab," so the building
blocks exist; this extracts the fetch+parse half into a reusable, tab-independent helper.

Everything else composes around that seam.

---

## Proposed design

### 1. Background prefetch kicker (active tab only)
- When `build_snapshot` computes `diff_stale = Some(pr_head…)` **for the active tab**, and no fresh
  `pr_open_cache` entry exists at the latest head_oid, kick a **debounced** background job (mirror
  `kick_deferred_tab_refresh` / the `run_blocking` worker pattern).
- Job calls the new `fetch_pr_diff_to_buffer` and writes the result into `pr_open_cache` (+ persist).
- **Gating (mandatory):** active tab only; one in-flight prefetch per tab (guard a
  `prefetching: HashSet<PrOpenCacheKey>` or per-tab flag); debounce so rapid poll ticks don't stack.

### 2. Two-state pill
Extend `DiffStaleSnapshot` with a status enum:
- `NewCommits` — detected, buffer not ready yet (fetching or not started). Label: *"New commits on
  origin — preparing…"* (or keep today's "Sync" affordance as a manual fallback).
- `DiffReady` — a fresh `pr_open_cache` entry exists at the latest head_oid. Label: **"New diff
  ready — click to view."**
- Compute the status by checking `cached_pr_open_entry` freshness against the latest cache head_oid.
- Frontend: `BranchContextBar.svelte` renders the second label + routes the click (see #3).

### 3. Cheap "apply staged" action
- New lightweight command `apply_staged_diff` (or a branch inside `force_refresh_diff`):
  - If a fresh buffer exists at the latest head → **swap cache → tab** (`raw_diff`, parsed files,
    `pr_commits`, set `last_diff_head_oid`). No git fetch. Instant.
  - Else → fall back to today's `force_refresh_diff` (fetch on click).
- The open path already builds a tab from a cached `raw_diff`; reuse that to populate the live tab.

### 4. `pr_commits` comes along for free
The background fetch should fetch commits too (so "New diff ready" carries the new commit list).
The commits-refresh wiring already exists from this cycle's fix — reuse it inside
`fetch_pr_diff_to_buffer`.

---

## Difficulty estimate

**Medium — ~1 focused day**, low-to-moderate risk (additive; the live tab is untouched until apply).
Materially de-risked by the cache/persistence/pill work already landed.

| Piece | Effort |
|---|---|
| `fetch_pr_diff_to_buffer` seam split (the crux) | Medium |
| Background prefetch kicker + active-tab/debounce gating | Small–medium |
| Two-state `DiffStaleSnapshot` + frontend label | Small–medium |
| `apply_staged_diff` (cache → tab swap) | Medium |
| Tests | Small–medium |

---

## Concurrency & correctness notes
- **Stale-of-stale:** origin can move from head N → N+1 while prefetching N. The head_oid cache key
  self-heals (the N entry simply won't match the new latest; next poll re-detects and re-prefetches).
  Add the in-flight guard so fetches don't stack.
- **Large diffs:** prefetch+parse must run on the worker-thread pattern (never under the App lock) to
  avoid UI jank. The 2 MB persist guard already bounds the disk side; in-memory is bounded by the
  32-entry cap.
- **Reviewer control preserved:** the diff never swaps without a click — this is the whole point and
  is correct by construction (apply is the only mutation of the live tab).

---

## Test plan (red→green)
1. **Seam:** `fetch_pr_diff_to_buffer` writes a fresh `pr_open_cache` entry at the new head_oid and
   does **not** mutate a passed-in/representative tab (assert tab's `files`/`last_diff_head_oid`
   unchanged; assert cache entry present with expected `raw_diff` + `pr_commits`).
2. **Pill status:** given a fresh buffer at latest head → `DiffStaleSnapshot.status == DiffReady`;
   given no buffer → `NewCommits`. Negatives: oids equal → `None`; unknown pr_number → `None`.
3. **Apply staged:** with a fresh buffer present, `apply_staged_diff` swaps files/`pr_commits` into
   the tab and clears the pill **without** any git fetch (assert no fetch side effect; assert tab
   now holds the buffer's files + new `last_diff_head_oid`). Without a buffer → falls back to fetch.
4. **Gating:** prefetch kicks only for the active tab; a second poll tick while one is in-flight does
   not spawn a second job.
5. **Manual e2e:** open a PR, push a commit, wait for pr_cache tick → pill shows "preparing…" then
   "New diff ready" → click → diff + COMMITS panel update instantly, pill clears.

## Rollback
Feature is additive and gated. Revert the prefetch kicker + the two-state pill + `apply_staged_diff`
to fall back to today's fetch-on-click Sync. The seam helper (`fetch_pr_diff_to_buffer`) is harmless
on its own.

## Non-goals
- Auto-applying the new diff (explicitly rejected — reviewer clicks to render).
- Prefetching non-active tabs.
- Reducing *total* gh/git calls (different lever; see "Honest scoping" above).
