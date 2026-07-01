# gh CLI rate-limit slimming (desktop background loops)

## In plain terms

The desktop app keeps three background timers running the whole time a PR tab is open: every
30s it asks GitHub four things at once (PR status, CI checks, comments, reviews), every 30s it
asks "did the PR branch move?", and every 45s it pulls review comments. None of them checked
whether the answer they already had was still fresh — they just re-asked on the clock. With one
PR tab open that was roughly **760–920 GitHub calls/hour**, and it multiplied per open tab.
GitHub's *secondary* rate limit punishes bursts and concurrency, and the 30s timer fired four
calls at the same instant — exactly what trips it.

This change makes each timer check freshness before it calls out ("is what I have recent enough?
then skip"), drops one purely-duplicated CI fetch, and collapses the four-call burst into two.
Steady state for one idle PR tab falls from ~900 calls/hr to well under ~50.

## What changed

- **30s PR-status loop now skips when fresh (≤90s).** The loop already computed a `last_updated`
  timestamp and never read it; it's now the freshness gate. The startup burst is gone too (the
  loop sleeps before its first fetch instead of after).
- **PR-status fan-out collapsed 4 → 2 subprocesses.** The overview, conversation-comments, and
  reviews calls were three separate `gh pr view --json` projections of the *same* PR object;
  they're now one `gh pr view --json <14 fields>` parsed three ways. `gh pr checks` stays a
  separate call (different subcommand). Verified the merged `comments`/`reviews` are byte-identical
  to the standalone calls.
- **Tab switches no longer re-burst.** `kick_active_gh_status` (fired on every tab open / switch /
  close) now returns early when the active tab's status is <10s old.
- **Comment-sync loop is throttled.** The 45s loop skips a tick when the PR's head commit hasn't
  moved since the last successful sync and that sync was <90s ago. Comments and reviews are posted
  independently of any push, so this is a poll throttle (bounding comment-panel latency to ~90s on a
  push-idle PR), not change-detection — a new push moves the head OID (detected within ≤60s) and
  drops through the throttle so freshly-pushed review threads sync promptly.
- **Dropped a duplicated CI fetch.** The comment-sync loop's local-clone PR-overview refresh was
  re-running `gh pr checks` every 45s — data the 30s status loop already owns. It now uses a
  checks-free overview variant (−~80 calls/hr per local-clone PR tab). Metadata (title / state /
  reviewers) is still refreshed.
- **PR-head probe throttled to 60s.** The 30s "did the PR head move on origin?" probe now skips
  when it probed the same PR <60s ago (the throttle re-arms on every probe, so it never silently
  reverts to firing every tick).
- **Cold PR open no longer fetches the diff twice.** Opening a PR for the first time fetched the
  diff once to build the tab, then `enter_pr_diff()` re-fetched the identical diff via
  `refresh_diff()`. A new `enter_pr_diff_freshly_loaded()` trusts the just-loaded diff and skips
  the redundant `gh pr diff` (with a fallback to a real refresh if the tab somehow has no files).

> **Not included (finding 7).** Caching the submit-time head OID for review submission was prototyped
> and then dropped: an adversarial review found the cached OID can be stale (the PR cache is persisted
> across sessions, so it can be far behind right after launch), and submitting a review against a stale
> commit can drop inline comments or anchor them to the wrong commit. It saves ~1 `gh` call per *manual*
> submit and nothing in steady state, so it isn't worth a write-path correctness risk. Review submission
> keeps fetching the head OID fresh, exactly as before. A safe version (anchor to the reviewed diff's
> own head, `last_diff_head_oid`) is left as a follow-up.

## Staleness tradeoffs (intentional)

| Surface | Before | After (worst case) |
|---|---|---|
| CI / review status | ≤30s | **≤90s** when idle |
| Auto-pull of teammate comments (no push) | ≤45s | **≤~90s** (poll throttle; a push resyncs within ~1–2 min) |
| "PR head updated on origin" stale pill | ≤30s | **≤60s** |
| Tab-switch status refresh | always | skipped if <10s old |

Manual sync (`G` / "sync now") is untouched and still immediate; a real push is detected within
≤60s and syncs on the next tick regardless of the 5-minute timer.

## Why it's safe

- The freshness gate is a pure `status_is_fresh()` that **fails open** — a missing, non-numeric,
  or future timestamp is treated as stale (fetch), never as fresh (skip) — with unit tests on the
  boundary and fail-open cases.
- The fan-out merge keeps every existing pure parser unchanged and adds a test proving the
  combined 14-field JSON extracts overview / comments / reviews independently (no cross-contamination).
  One accepted tradeoff: overview + comments + reviews now share a single subprocess, so a failed call
  yields no snapshot rather than a partial one — but a warm tab keeps its last-good snapshot (the cache
  isn't cleared on a failed tick), and cold-open already required the overview call to succeed, so the
  practical behavior is unchanged while total call volume drops.
- The comment-sync gate carries the gate-time head OID forward so a push landing mid-fetch isn't
  mistaken for "already synced," and falls open on an unknown OID.
- The cold-open diff skip uses a dedicated entry point (not a blanket `files.is_empty()` guard
  inside `enter_pr_diff`, which would have shown a wrong-scope diff when switching modes); tests
  cover skip / fallback / always-refresh.
- The submit head-OID cache returns `None` on miss or empty OID, so a stale/guessed OID is never
  substituted into a submission; `submit_review`'s existing 422 fallback still applies.

## Implementation

- `crates/er-desktop/src/gh_status_cache.rs` — new pure `status_is_fresh()` / `now_epoch_secs()`
  helpers + tests.
- `crates/er-desktop/src/main.rs` — the three background loops gain freshness / OID / throttle
  gates; `cached_head_oid()` helper; pure `comment_sync_recently_synced()` / `probe_recently_done()`
  gate predicates with unit tests.
- `crates/er-desktop/src/commands.rs` — `kick_active_gh_status` 10s gate; `fetch_github_status`
  4→2 subprocesses; cold-open uses `enter_pr_diff_freshly_loaded()`.
- `crates/er-engine/src/github.rs` — new `gh_pr_status_remote()` / `PrStatusBundle`;
  `gh_pr_overview_no_checks()`.
- `crates/er-engine/src/sync.rs` — comment-sync uses the checks-free overview for local clones.
- `crates/er-engine/src/app/state/mod.rs` — `enter_pr_diff` split into `enter_pr_diff_impl` +
  `enter_pr_diff_freshly_loaded`; tests.

Design doc: `internal-docs/gh-rate-limit-slimming.md`. Findings 1–6 land here; finding 7 was reverted
after review (see above) and the structural `run_gh` chokepoint (finding 8) is a separate follow-up.
