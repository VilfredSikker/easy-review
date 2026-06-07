# Unified View Modes, Isolated Review Buckets & Diff Performance

> Status: planned (v2 — revised after adversarial review). Supersedes the storage
> follow-ups in [`plan-tui-desktop-ai-storage.md`](plan-tui-desktop-ai-storage.md).
> **Risk level: 2 (Orange)** — review-data storage *identity* + per-view isolation change.
> Canonical PR identity resolved (own `owner/repo` from `origin`; fork PRs out of scope) and
> History bucket resolved (own bucket). Challenger review complete.

## Context

`er` exposes diffs through two overlapping axes today:

- **`DiffMode`** (`Branch | Unstaged | Staged | History | Conflicts | Hidden`, `state/mod.rs:63-70`) — chooses the `git diff` command.
- **`DiffSource`** (`Pr | Origin | Local`, `state/mod.rs:211-215`) — chooses what the *Branch* diff compares against.

Review artifacts (`reviewed`, `questions.json`, `github-comments.json`, AI `review.json`) are stored **per branch only** — blind to mode and source. Consequences the user hit:

1. **Desktop Local-branch questions don't appear inline.** *Root cause UNCONFIRMED* — the originally-suspected "cached snapshot" mechanism was refuted on review (see Phase 3 spike).
2. **"Origin" vs "PR Diff" is ambiguous** — both compare a branch against a base.
3. **No isolation** — reviewing / commenting in one view shows up in every other view of the same branch (`save_reviewed_files` → single `reviewed_path`, `paths.rs:36-42`; `reload_ai_state` per-branch, `state/mod.rs:2601-2627`).
4. **Reviewed click is janky** — `toggle_reviewed` rebuilds the entire snapshot (`commands.rs:746-768`; `snapshot.rs:1001/1057`).
5. **Sluggish** — overscan 5, lazy concurrency 2, ~540–940 ms save→UI latency.
6. **Mode switching never reloads storage.**

### Decisions (confirmed with user)

- **Remove `Origin`.** PR Diff must equal the GitHub PR view (PR head vs base = old `DiffSource::Pr`).
- **Promote PR Diff to a first-class view.** Collapse `DiffSource` into the view axis. Unified views for TUI + Desktop: **Branch · Unstaged · Staged · PR Diff · History**. `--remote` opens a tab where only PR Diff is available.
- **Per-view isolated buckets:** `branch`, `unstaged`, `staged`, `pr` (per PR #), and `history` (its own bucket — confirmed). A comment on the unstaged diff must not surface in branch view.
- **`view-buckets/` layout, no data migration.** Old flat per-branch files are left orphaned. New buckets start clean. Drops migration/rollback risk to zero.
- **PR bucket shared** between a local clone's PR Diff view and `er --remote`, keyed on `owner/repo` (below).
- **Annotations isolated by default; unified on demand (no promotion).** A `bucket:` filter (Phase 5) widens visibility across views, anchored by content — data is never moved as code flows unstaged → staged → committed → pushed. GitHub comments are *creatable* only in PR Diff (pushable) and *mirrored* read-only in local views. Findings stay bucket-local and go stale ("addressed?").
- **Canonical PR identity = `owner/repo` parsed from `origin`** (slugified to `owner-repo`, matching `--remote`). A cheap local URL parse — no `gh` call, works offline. The current blocker is only that `slug_repo` returns the URL **basename** (`easy-review`) while `--remote` uses `owner-repo`; the fix is to parse `owner/repo` (SSH `git@host:owner/repo.git` + HTTPS, case pinned to `slugify`). **Fork PRs are an explicit non-goal** — if `origin` is a fork, its `owner/repo` won't match the upstream base, so the bucket won't be shared; documented as a known limitation (user does not review fork PRs).

### In plain terms

- **The problem:** Notes/checkmarks bleed between views, local questions don't show, a checkbox click freezes the UI, scroll flashes unformatted text.
- **Why it matters:** You can't trust where your review notes live, and the app feels laggy.
- **The fix:** One clean view list, each view its own notebook; show notes instantly; don't rebuild the screen for a checkbox; render ahead and watch files faster.
- **TL;DR:** One view axis, isolated per-view notes, snappier diff — with the PR bucket pinned to the *base* repo so a clone and `--remote` agree.

---

## Architecture: one view axis, one bucket per view

### View model (engine; used by TUI + Desktop)

| View | Diff command | Bucket | Available when |
|------|--------------|--------|----------------|
| **Branch** | `base...HEAD` (local) | `branch` | local checkout |
| **Unstaged** | `git diff` (working tree vs index) | `unstaged` | local checkout |
| **Staged** | `git diff --cached` | `staged` | local checkout |
| **PR Diff** | PR head vs base (== GitHub) | `pr` (per PR #, base repo) | branch has a PR, or `--remote` |
| **History** | per-commit `git show` | `history` | local checkout |

`DiffMode` gains `PrDiff`. `DiffSource`/`Origin` and the network-fetch logic from `set_diff_source(Pr)` (`state/mod.rs:1850-1858`) move into PrDiff's diff fetch — **with caching + an error channel** (see below).

### Storage layout (`view-buckets/`, no migration)

```
~/.local/share/easy-review/repos/
  ├─ <slug_repo>/branches/<branch_slug>/view-buckets/
  │     ├─ branch/      { reviewed, questions.json, github-comments.json, review.json, … }
  │     ├─ unstaged/
  │     ├─ staged/
  │     └─ history/
  └─ <base_owner_repo>/prs/pr-<N>/   { reviewed, questions.json, github-comments.json, … }
```

- **Local sub-buckets** under `branches/<branch_slug>/` (keyed by the clone's `slug_repo`, `storage.rs:50-75`) so they follow the branch.
- **PR bucket** under `repos/<owner_repo>/prs/pr-<N>/`. `<owner_repo>` is parsed from `origin` (slugified `owner-repo`), resolved identically for a local clone and `er --remote`. **There is no reusable parser today** — `github.rs:parse_github_pr_url` is PR-URL/HTTPS-only and there is no origin→`owner/repo` function; write one handling SSH (`git@github.com:owner/repo.git`) and HTTPS, and pin the case policy to match `slugify` (the desktop's `normalize_remote_slug` lowercases — do not mix). Fork PRs (origin = fork) are a documented non-goal.

### Bucket resolution (single source of truth)

```rust
// crates/er-engine/src/app/state/mod.rs
enum ReviewBucket { Branch, Unstaged, Staged, History, Pr }

fn review_bucket(&self) -> ReviewBucket {
    match self.mode {
        DiffMode::PrDiff   => ReviewBucket::Pr,
        DiffMode::Unstaged => ReviewBucket::Unstaged,
        DiffMode::Staged   => ReviewBucket::Staged,
        DiffMode::History  => ReviewBucket::History,
        _                  => ReviewBucket::Branch,
    }
}
```

`apply_managed_root()` (`state/mod.rs:1682-1724`) resolves `Pr` → `repos/<owner_repo>/prs/pr-<N>/`; everything else → `repos/<slug_repo>/branches/<branch_slug>/view-buckets/<bucket>/`. All sidecar paths (`reviewed_path`, `er_dir`, `comments_dir`, questions/github-comments) derive from the single resolved `ErRoot` (`paths.rs:24-42`, `state/mod.rs:1672-1679`) — confirmed: no per-file hardcoded paths, so no `paths.rs` structural change is needed. (Also fix the stale "uses cache dir in remote mode" docstrings at `state/mod.rs:1786, 2630` and `comments.rs:329`.)

### PR Diff mode: network + errors + caching

Entering PrDiff needs `fetch_pr_head` + base ref resolution (network/`gh`). Today `set_mode` returns `()` and swallows refresh errors (`let _ = self.refresh_diff_mode_switch()`, `state/mod.rs:3312`). Change the PR-entry path to **return `Result`** (e.g. a dedicated `enter_pr_diff()`), surface failures as a toast (desktop already shows a `switching` spinner, `app.svelte.ts:515-516`), and **cache the resolved PR head/base refs** so re-entering PR Diff is instant and offline-tolerant.

### Mode-switch correctness (quirk #6)

Switching among the five views is one `set_mode()` call. `set_mode` already restores selection by path and resets hunk/line/scroll (`state/mod.rs:3314-3328`) — **keep that** (resetting to 0 is a regression). Add: when `review_bucket()` changes, `apply_managed_root()` + reload `reviewed` + `reload_ai_state()` for the new bucket **before** the path-restore. Do **not** `{#key}`-remount the desktop view (destroys the virtual window + highlight cache → more jank); reset scroll/spans explicitly.

---

## Implementation phases

### Phase 1 — Engine: buckets + PrDiff mode (DiffSource kept as a shim)

**Goal:** per-bucket storage, canonical PR identity, PrDiff diff fetch, reload-on-switch — **without deleting `DiffSource`**, so the workspace keeps compiling and Phase 1 is independently mergeable/testable. (Deleting `DiffSource` here would break `er-desktop` — see Phase 3.)

| File | Action | Change |
|------|--------|--------|
| `crates/er-engine/src/app/state/mod.rs` | modify | Add `DiffMode::PrDiff`, `ReviewBucket` + `review_bucket()`. Add `enter_pr_diff() -> Result` (fetch + cache PR head/base; reuse `set_diff_source(Pr)` logic at 1850-1858). In `set_mode`, on bucket change → `apply_managed_root()` + reload `reviewed` + `reload_ai_state()`, then keep path-restore. Add `visible_modes()` (1642-1665) handling for PrDiff. |
| `crates/er-engine/src/storage.rs` | modify | `view_bucket_dir(slug_repo, branch_slug, bucket)`; `pr_bucket_dir(owner_repo, pr_n)` → `prs/pr-<N>`; `canonical_repo_slug(repo_root)` → `owner-repo` from `origin` (SSH+HTTPS, case-pinned). |
| `crates/er-engine/src/github.rs` | modify | New `parse_origin_remote() -> owner/repo` (SSH + HTTPS). No `gh` call needed for identity (fork PRs out of scope). |
| `crates/er-engine/src/app/state/mod.rs` | modify | `apply_managed_root` resolves bucket dir; `save_reviewed_files` (3526) — **lift the `is_remote()` early-return (3527) for the `pr` bucket** so remote PR reviewed persists to the shared dir. `finish_storage_setup`/`sync_managed_storage` (1727-1745), `reload_ai_state` (2601), `storage_branch_scope` (1773) consume the bucket dir. |

**Verify:** `cargo test -p er-engine` — `review_bucket()` mapping; distinct dirs per bucket; `canonical_repo_slug` parity for SSH vs HTTPS forms of the **same** origin (→ same `owner-repo`, matching `--remote`); mode switch reloads a different `reviewed` set; remote PR now writes `reviewed`.

### Phase 2 — TUI: expose PR Diff, route `--remote` to the shared bucket

| File | Action | Change |
|------|--------|--------|
| `crates/er-tui/src/main.rs` | modify | Add PR Diff to the mode cycle (gate on PR presence); `--remote` builds a PrDiff-only tab via `enter_pr_diff`. |
| `crates/er-engine/src/app/state/mod.rs` | modify | `new_remote*` sets `mode = PrDiff`, resolves the `pr` bucket via base-repo slug. |
| `crates/er-tui/src/ui/status_bar.rs` | modify | Show the new view label. |

**Verify:** `er --remote <url>` and a local clone's PR Diff write the **same** `prs/pr-<N>/`; reviewing in PR Diff doesn't touch `view-buckets/branch/reviewed`.

### Phase 3 — Remove `DiffSource` (engine + desktop together) & wire desktop views

**Goal:** delete the now-shimmed `DiffSource` in one coordinated change across engine + desktop (they must move together to compile), and surface the 5-view selector.

| File | Action | Change |
|------|--------|--------|
| `crates/er-engine/src/app/state/mod.rs` | modify | Delete `DiffSource`, `diff_source()`, `available_diff_sources()`, `set_diff_source()` (211-215, 1792-1883). Remove orphans: `local_branch_diff_ref` threading (2122-2126), `has_upstream` probe (2185-2198), `ahead_behind_vs_upstream`, `fetch_branch_upstream_into_er_ref`/`fetch_remote_base_ref_for_diff`. |
| `crates/er-desktop/src/snapshot.rs` | modify | Remove `build_diff_source_snapshot` (2484), `build_diff_source_copy` (2557), `DiffSourceSnapshot`, and the `diff_source` field on `AppSnapshot` (1309). Bucket-aware `build_hunks`/per-file `reviewed` (1090) + `reviewed_count` (1036). |
| `crates/er-desktop/src/commands.rs` | modify | Remove `set_diff_source` command. Add `"pr_diff"` arm to `set_mode` (731-738) **and** `feature_allows_mode_str` (≈715). `add_question` (1513): see Phase 3 spike. |
| `desktop-ui/src/lib/components/BranchContextBar.svelte` | modify | Remove `switchDiffSource` (35), `diffSourceLabels` (45), `segLabels` (52), `switchingSource`, and the segmented markup (183-209). |
| `desktop-ui/src/lib/components/ScopeSelector.svelte` | modify | Single 5-view selector; gate working-tree views on `local_branch_checked_out`, PR Diff on PR presence. |
| `desktop-ui/src/lib/types/*` | modify | Drop `DiffSourceSnapshot`; add PrDiff to the mode union. |
| `desktop-ui/src/lib/components/DiffView.svelte` | modify | On mode change reset scroll + clear `_spansAppliedKeys` (no remount). |

**Phase 3 spike (quirk #1 — do BEFORE writing a fix):** Reproduce in the running desktop app: add a question in Local/Unstaged, then inspect whether `snapshot.ai.threads` and/or `file.hunks[].threads` actually contain the new question immediately after `add_question` resolves (the full-snapshot path *should* carry it; `submit_comment_text` reload + `ingestCommandSnapshot` apply it without poll). The likely real cause is **frontend reactivity** — `annotationIndex` is `$derived` from `snapshot.ai` + the `files` prop (`FlatDiffView.svelte:131-140`); if a same-identity `files` array is re-passed the `$derived` may not recompute. Confirm the layer, then fix that (e.g. ensure new array identity / explicit invalidation). Do **not** ship the `hunk.threads[]` injection — it's a no-op.

**Verify:** all 5 views shown, no Origin; question appears inline instantly and only in its view; reviewed isolation across views.

### Phase 4 — Performance

| File | Action | Change |
|------|--------|--------|
| `crates/er-engine/src/app/state/mod.rs` | modify | `reviewed_revision: u64`, bumped only on reviewed changes. |
| `crates/er-desktop/src/commands.rs` | modify | `toggle_reviewed`/`mark_reviewed` (746-768): chrome-only response (bump `chrome_revision` + `reviewed_revision`, no hunk/AI rebuild) **plus a minimal `{path, reviewed}` delta** so the clicked row's checkmark updates — the per-file `reviewed` flag lives in the omitted `files` list (`snapshot.rs:1090`), so chrome-only alone leaves it stale (R3). Add `reviewed_revision` to `PollResponse` (`poll` 614). |
| `desktop-ui/src/lib/stores/app.svelte.ts` | modify | On chrome-only reviewed update, apply the `{path, reviewed}` delta to the existing `files` (don't replace snapshot); skip `$derived` rebuild (302-379). |
| `desktop-ui/src/lib/components/FlatDiffView.svelte` | modify | `OVERSCAN` 5→15 (266); `REQUEST_FILE_CONCURRENCY` 2→4 (330); gate `evictSpanKeysForPath()` on `cache_key` change (358). |
| `crates/er-desktop/src/main.rs` | modify | Watch debounce 500→250 ms (`FileWatcher::new`, 947). **Keep** the single emitter thread (1260-1286) that mirrors `desktop_revision` → `emit("er://revision", …)`: an 80 ms atomic-load loop with a `current != last_emitted` idle guard (near-free when idle) + a 40 ms coalesce. Do **not** convert to pure condvar (R4). For snappier local, optionally **lower** the 80 ms detection poll to ~40 ms (do *not* raise it — that adds latency). |
| `desktop-ui/src/lib/diffRenderModel.ts` | modify | **File-collapse lag:** `applyCollapsedFiles` (771) does a full O(N-rows) rebuild of `rows` + `cumulativeOffsets` + 4 index Maps on **every** `diffFileCollapse.revision` bump (FlatDiffView:154-157), uncached. **Memoize** by `(baseModel.identity + collapsed-set signature)` so repeated collapse/expand is O(1); the base model is already LRU-cached (674), the collapsed derivation is not. |
| `desktop-ui/src/lib/components/FlatDiffView.svelte` | modify | **File-collapse lag (cont.):** collapse must be a pure layout change — the new row set currently forces `_spansAppliedKeys` re-validation (504-521) → re-highlight flicker on still-visible rows. Skip span eviction/re-apply for rows unchanged by collapse; collapse never re-fetches or re-highlights content. (Longer-term option: model collapse as a height-0/skip in geometry instead of rebuilding the row array, keeping row identity stable.) |

**Verify:** reviewed click `<100 ms`, clicked checkmark updates immediately, poll returns the chrome delta (no hunk rebuild); no plain-text flash scrolling a 50-file diff; **collapsing/expanding a file in a 100+ file diff is instant with no re-highlight flicker**; save→UI `<~350 ms`. `ER_DESKTOP_PROFILE_POLL=1`.

**Watch→UI latency chain** (the snappy-local target): file save → notify debounce (`500→250 ms`) → `refresh_diff_quick` + `bump_desktop_revision("watcher_refresh")` (`main.rs:1011`) → ≤80 ms (or ~40 ms) emitter detect → 40 ms coalesce → `emit("er://revision")` (`main.rs:1272`) → frontend `poll()`. Post-change budget ≈ **<350 ms** (was 540–940 ms). The 30 s `poll()` fallback (`app.svelte.ts:143`) only matters if an event is dropped.

### Phase 5 — Cross-bucket annotation visibility (optional, post-core)

**Goal:** annotations stay **isolated by default** (no bleed) but are never lost or hidden as code flows unstaged → staged → committed → pushed. One mechanism — "show another bucket's annotations in this view, anchored by content" — serves two workflows: (a) addressing **PR feedback** while editing locally, and (b) finding/editing/removing **unstaged/staged notes** after they're committed. **No data is moved, promoted, or duplicated** (user decision: cross-bucket filter, not promotion).

**Mechanism**
- Extend the composable filter (`filter.rs`, `f` key) with a **`bucket:` dimension** (`unstaged|staged|branch|pr|all`). **Default = current view only** → isolation preserved. Widen on demand.
- **Anchor by content, not line number** — match `line_content` against the active diff (exact → trimmed → nearest-in-hunk), reusing `relocate_all_comments()` + `CommentIndexData`. Show an **origin badge** ("from: unstaged"). Reinterpret staleness as **"addressed?"** when the line changed since the note.

**Per-type rules**
- **Questions** (private): editable/removable from any view via the filter; edits route to the *owning* bucket's `questions.json`. No auto-move.
- **GitHub comments**: **creatable only in PR Diff** (needs a pushable target). In local views they appear **read-only** (the "PR feedback" overlay = the `bucket:pr` case). Reply/resolve from a local view is disabled or queued — never a silent no-op.
- **Findings** (AI): **bucket-local always** — per-review snapshots, *not* pulled cross-bucket even with `bucket:all`; go stale → "addressed?" when their line changes. Re-run AI per view for fresh findings.
- Behind `features.pr_feedback_overlay` (default off) + an inline-layer toggle, consistent with `show_github_comments`.

**Workflow payoff:** pull PR comments (`G`) → fix on Branch/Unstaged → flagged lines show feedback → editing flips to "addressed" (`addressed N/M`). And a question made in Unstaged survives commit+push: flip `bucket:all` in Branch to see/edit/remove it on the committed line.

| File | Action | Change |
|------|--------|--------|
| `crates/er-engine/src/app/filter.rs` | modify | Add a `Bucket` filter rule (`bucket:unstaged\|staged\|branch\|pr\|all`); parse + apply; add to history/presets. |
| `crates/er-engine/src/app/state/mod.rs` | modify | On widened bucket scope, load other buckets' questions + the `pr` bucket's github-comments **read-only**, anchor via `relocate_all_comments()`/`CommentIndexData`, tag origin; route edits to the owning bucket; `addressed N/M`. Findings excluded. |
| `crates/er-engine/src/ai/review.rs` | modify | `origin_bucket` tag + anchor-state (open/addressed/unanchored) on `CommentRef`; findings not anchored cross-bucket. |
| `crates/er-engine/src/config.rs` | modify | `features.pr_feedback_overlay` flag (default off) + settings item. |
| `crates/er-tui/src/ui/diff_view.rs` + `status_bar.rs` | modify | Origin badge, "addressed?" styling, bucket-scope chip, read-only PR affordance, "N not shown here" chip. |
| `crates/er-desktop/src/snapshot.rs` + `desktop-ui/src/lib/components/FlatDiffView.svelte` | modify | Same overlay + badge + filter control; edits routed to the owning bucket. |

**Verify:**
- PR comments: pull → Branch → anchor on lines; edit a flagged line → "addressed"; Unstaged shows only comments on touched lines; **no write-back** to the `pr` bucket; GitHub-comment creation unavailable outside PR Diff.
- Lifecycle: add a question in Unstaged → commit + push → Branch default hides it → `bucket:all` shows it on the committed line with "from: unstaged" badge → edit/remove from Branch lands in the unstaged bucket's `questions.json`.
- Findings: an unstaged finding does **not** appear in Branch even with `bucket:all`; shows "addressed?" if its line changed.

**Why optional/post-core:** depends on the bucket model (Phases 1–3); pure additive UX; ships after the storage + perf core.

---

## File change summary

| File | Action | Description |
|------|--------|-------------|
| `crates/er-engine/src/app/state/mod.rs` | modify | `DiffMode::PrDiff`; `ReviewBucket`+`review_bucket()`; `enter_pr_diff()`; reload-on-switch; bucket save/load; lift remote `save_reviewed` guard; `reviewed_revision`; Phase 3 deletes `DiffSource` + orphans. |
| `crates/er-engine/src/storage.rs` | modify | `view_bucket_dir`, `pr_bucket_dir`, `canonical_repo_slug` (`owner-repo` from `origin`). |
| `crates/er-engine/src/github.rs` | modify | `parse_origin_remote` → `owner/repo` (SSH+HTTPS). No `gh` call for identity. |
| `crates/er-engine/src/app/filter.rs` | modify | (Phase 5) `Bucket` filter rule for cross-bucket annotation visibility. |
| `crates/er-engine/src/paths.rs` | none | Confirmed no change — sidecar paths derive from `ErRoot`. (Fix stale docstrings only.) |
| `crates/er-tui/src/main.rs` | modify | PR Diff view; `--remote` → PrDiff-only tab. |
| `crates/er-tui/src/ui/status_bar.rs` | modify | New view label; (Phase 5) bucket-scope chip. |
| `crates/er-engine/src/ai/review.rs` | modify | (Phase 5) `origin_bucket` + anchor-state on `CommentRef`; findings excluded from cross-bucket. |
| `crates/er-engine/src/config.rs` | modify | (Phase 5) `features.pr_feedback_overlay` flag + settings item. |
| `crates/er-tui/src/ui/diff_view.rs` | modify | (Phase 5) origin badge, "addressed?" styling, read-only PR affordance. |
| `crates/er-desktop/src/commands.rs` | modify | `set_mode(pr_diff)` + feature gate; remove `set_diff_source`; chrome-only reviewed + delta; quirk-#1 fix (post-spike); `reviewed_revision`. |
| `crates/er-desktop/src/snapshot.rs` | modify | Remove DiffSource snapshot/copy + field; bucket-aware build. |
| `crates/er-desktop/src/main.rs` | modify | 250 ms debounce; keep bounded emitter poll. |
| `desktop-ui/src/lib/components/BranchContextBar.svelte` | modify | Remove source control + helpers. |
| `desktop-ui/src/lib/components/ScopeSelector.svelte` | modify | 5-view selector with gating. |
| `desktop-ui/src/lib/components/DiffView.svelte` | modify | Reset scroll/spans on mode change (no remount). |
| `desktop-ui/src/lib/components/FlatDiffView.svelte` | modify | Overscan 15, concurrency 4, conditional span eviction; quirk-#1 reactivity fix. |
| `desktop-ui/src/lib/stores/app.svelte.ts` | modify | Chrome-only reviewed delta handling. |
| `desktop-ui/src/lib/types/*` | modify | Drop `DiffSourceSnapshot`; add PrDiff mode. |
| `CLAUDE.md` | modify | Document the unified view axis + bucket layout + base-repo PR identity. |

---

## Risks & rollback (post-review)

Ranked, from the challenger pass:

1. **Canonical PR slug (Medium, was High).** Shared-bucket correctness depends on a clone and `--remote` producing the same `owner-repo`. *Mitigation:* parse `owner/repo` from `origin` (SSH+HTTPS, pinned case), test SSH-vs-HTTPS parity. **Fork PRs (origin = fork) are an accepted non-goal**, not engineered around — documented limitation; downgrades this from the prior silent-divergence hazard.
2. **PR-Diff-as-mode network I/O in sync `set_mode` (High).** Blocking + silent failure. *Mitigation:* `Result`-returning `enter_pr_diff`, toast on failure, cached refs.
3. **Phase-1 sequencing (Medium).** Deleting `DiffSource` early breaks `er-desktop`. *Mitigation:* keep the shim; remove in Phase 3 (engine+desktop together). Rollback is per-phase except Phase 3, which is one atomic engine+desktop change.
4. **chrome-only stale checkmark (Medium).** `reviewed` flag is in the omitted `files`. *Mitigation:* `{path, reviewed}` delta / optimistic client toggle.
5. **Remote reviewed never persisted today (Medium).** `save_reviewed_files` returns early when remote. *Mitigation:* lift the guard for the `pr` bucket; test.
6. **Quirk #1 cause unconfirmed (Medium).** Suspected fix is a no-op. *Mitigation:* reproduce-first spike (Phase 3) before coding.
7. **Revision-watcher rewrite scope (Medium).** `desktop_revision` is bumped from **dozens** of sites — most mutating commands plus ~8 background loops (`watcher_refresh` 1011, gh-status 816, comments 909, pr-cache 1066/1090, meta 1124-1144, remote-pr 745, tab-warmer 1199) via `fetch_add`/`bump_desktop_revision`. A single emitter thread (1260-1286) mirrors it to `er://revision`. Pure event-driven would require signalling from every bump site; one missed signal → 30 s stall. *Mitigation:* keep the cheap mirror poll (optionally lower 80→40 ms for latency, never raise).
8. **History bucket (resolved).** Own `history` bucket (confirmed) — no bleed into branch. Still verify auto-unmark (`auto_unmark=false` in `refresh_diff_mode_switch`, mod.rs:2017) can't fire against the branch set while in History.
9. **Two-bucket files double review effort (Low).** Unstaged + Staged are separate by design — confirmed intent; noted so it's a decision, not a surprise.
10. **No migration (accepted).** Old flat data orphaned; users see empty buckets initially. *Mitigation:* one-time notice; optional `er --clean-legacy` later.

**Rollback:** Phases 1, 2, 4 revert independently. Phase 3 is the one atomic engine+desktop change (DiffSource removal). `ER_REPO_LOCAL=1` remains for debugging.

---

## Verification (end-to-end, by quirk)

1. **#1 inline questions:** *spike-confirm cause first*; then Unstaged → add question → appears instantly; switch to Branch → absent; on disk under `…/view-buckets/unstaged/questions.json`.
2. **#2 Origin gone:** only `Branch/Unstaged/Staged/PR Diff/History`; PR Diff == `gh pr diff`.
3. **#3 isolation + sharing:** reviewed in Branch vs PR Diff each only in its own view; a clone (origin = the PR's repo) and `er --remote <url>` resolve the same `prs/pr-N/` and share reviewed. (Fork PRs out of scope.)
4. **#4 reviewed jank:** click → `<100 ms`, checkmark updates immediately, poll returns chrome delta, no hunk rebuild.
5. **#5 perf:** 50-file diff scroll, no plain-text flash; file edit → UI `<~350 ms`.
6. **#6 swap correctness:** switch among all 5 views → diff/file-panel/commit-view swap; selection restored by path; unstaged/staged shown only on local checkout; no stale notes/checkmarks; PR-entry failure shows a toast, not a silent stale diff.
7. **Cross-bucket (Phase 5):** question added in Unstaged survives commit+push and is visible/editable in Branch via `bucket:all` (origin badge); PR comments mirror read-only in local views and flip to "addressed" on edit; findings never cross buckets.
