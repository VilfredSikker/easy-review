# Easy Review — Desktop App: Feature List & Architecture Overview

> Scope: the **desktop app only** (`crates/er-desktop` Tauri backend + `desktop-ui`
> Svelte frontend). The TUI shares the engine but is a separate, simpler surface.
>
> Purpose: a design/planning reference for preparing new designs and thinking
> about the future feature set. The authoritative description of current behavior
> is always `CLAUDE.md` and the code itself — when this doc and the code disagree,
> the code wins.

## 1. What it is

A native desktop app for **reviewing git diffs in the age of AI-written code**. The
thesis: AI generates code faster than humans can review it, so the review surface
must be fast, live-updating, and able to *orchestrate AI reviewers* rather than just
display a diff. The app is a **viewer + orchestrator** — it never runs models
itself; it shells out to your local `git`, `gh`, and `claude`/agent CLIs and renders
what they produce. No backend, no telemetry.

## 2. Architecture at a glance (for design context)

- **Backend (Rust/Tauri):** owns all truth. A single `App` state behind a mutex,
  exposed through ~150 Tauri commands. Heavy work (git, gh, agents) runs off the main
  thread and on background threads; results surface through a **revision-event +
  snapshot model** (see frontend).
- **Frontend (Svelte):** a consumer of a single **`AppSnapshot`** contract. The
  backend emits an `er://revision` event on every state change; the frontend responds
  by calling `poll` to fetch the new snapshot (with a 30s safety-net poll as a
  fallback for missed events) and only re-renders when a revision number changes. The
  frontend owns only ephemeral UI prefs (panel widths, drawer heights, view mode) in
  localStorage.
- **Rendering specializations:** windowed/virtualized diff rendering, client-side
  syntax highlighting via **Shiki in a Web Worker** (plain text crosses IPC, spans
  are filled in-browser), differential snapshots (unchanged files omit their
  payload), and per-file content-hash cache keys.
- **Theming:** 8 shared themes driving both UI and syntax colors via CSS
  custom-property tokens. **All color flows through semantic tokens** — relevant if
  you're redesigning (never raw hex / stock Tailwind palette classes).

## 3. Screen anatomy (the shell)

```
┌────────────────────────────────────────────────────────────────┐
│ Title bar / Tab strip   (working tree · local branch · PR tabs) │
│ Branch context bar      (branch ← base, scope selector, status) │
├──────────┬──────────────────────────────────┬──────────────────┤
│  Left    │   Center                         │  Right rail       │
│  sidebar │   File tree + Diff view          │  "Review hub"     │
│          │   (or Browser / Agent output /   │  Branch / Review  │
│  Projects│    Export / Settings views)      │  / Notes tabs     │
│  Inbox   │                                  │  (collapsible)    │
├──────────┴──────────────────────────────────┴──────────────────┤
│ Terminal drawer (bottom, resizable PTY)                         │
│ Bottom hints · Background tasks · Toasts                        │
└────────────────────────────────────────────────────────────────┘
```

Plus full-surface overlays: **Command palette (⌘K)**, **AI action palette (⌘A)**,
**PR URL modal (⌘⇧O)**, **Arena launcher / running panel / results overlay**,
professor-focus and promote modals.

---

## 4. Full feature list (by area)

### A. Navigation & workspace model

- **Multi-tab workspace** — three tab kinds: **working tree**, **local branch**
  (read-only branch view), **remote PR** (read-only, no checkout). Tabs persist
  across restarts (`tabs.json`); active tab loads eagerly, others as lazy stubs.
- **Tab actions** — new tab (⌘⇧T), close (⌘W), switch by number (⌘1–9), drag to
  reorder.
- **Left sidebar** — New review, fuzzy **search across projects/branches/PRs**, and a
  per-project tree.
- **Projects** — each repo is a project (add via Open worktree, ⌘O). Per-project
  3-dot menu: **New tracked branch**, **Sync PR list**, **Delete**. Projects can be
  **remote-only** (no local clone).
- **Branch & PR lists per project** — tracked local branches, **My PRs**, **PRs to
  review** (others', not yet approved, max 5), **Saved/pinned PRs**, **Recent PRs**,
  **Recently merged** (max 5). Click opens (replaces tab; ⌘-click = new tab).
  **Hover pre-fetches** the PR diff so the click feels instant.
- **Scope selector** (branch context bar) — switch the diff scope: **Branch vs
  base · Unstaged · Staged · Commit history · Conflicts · Hidden/ignored**, each with
  live +/− counters. A PR-diff toggle appears when a PR is detected for the branch.
- **Command palette** — fuzzy command + file jump, with keyboard hints (`:` commands,
  `/` files, `@` symbols).

### B. Diff viewing

- **Unified & split diff** (toggle `d`), line numbers, soft-wrap, configurable tab
  width.
- **Windowed rendering** — only visible rows are built; handles very large diffs.
- **Lazy file loading** — big diffs ship file headers only; a file's hunks load when
  it scrolls into view.
- **Auto-compaction** — lock files, generated/minified code, and >500-line files
  collapse to expandable stubs.
- **Syntax highlighting** — Shiki in a Web Worker, per-file LRU cache, theme-matched.
- **Word-level diff** — intra-line add/del highlighting merged with syntax spans.
- **Diff search** (⌘F) — in-diff find, prefilled from the current text selection.
- **Code-reference highlighting** — click/hover an identifier to highlight its
  usages; a reference ruler + usages popover.
- **File tree** — collapsible, risk indicators, per-file finding/comment/question
  counts, reviewed state, status icons. Navigate with `j`/`k`; jump to next
  unreviewed.
- **Sticky file-path header**, hunk headers, fold rows.
- **Live watch mode** — auto-refreshes on edits/staging/commits; reviewed files
  auto-unmark when their diff changes. Manual refresh `R`; **force refresh**
  (re-fetch PR head/base from remote) ⌘R.

### C. Review tracking

- Mark files **reviewed** (Space), reviewed/total counter, filter to unreviewed,
  **jump to next unreviewed**.
- **Composable filters** — glob/status/size/risk rules, presets + history.
- **Recency sort** of files.

### D. AI review (single-agent)

- **Reviewers:** General + 8 domain experts (**Security, Performance, Reliability,
  Testing, API/contracts, Patterns, Simplifying, Mentorship**) + **Professor**
  (teaching/explainer) + **Triage** (fast routing scan).
- **AI action palette (⌘A)** — launch any reviewer over a scope (branch / current
  view / selected files).
- **Inline findings** — rendered in the diff at their line, severity-tagged
  (high/med/low), agent-labeled. The right-rail **Review tab** lists all findings,
  **filterable by agent**, with per-agent markdown summaries.
- **Triage card** — first impression, verdict, recommended experts, confidence,
  priority files.
- **Per-finding actions:** **Ask AI** (spawns a thread, async reply), **promote to
  GitHub comment**, **dismiss**, reply, validation responses stored on the finding.
- **Validate / re-anchor** — re-runs anchoring so findings & comments survive diff
  changes; staleness detection dims out-of-date AI data (SHA-256 diff hashing).
- **Model/provider/effort selection** — pick provider + model, Claude effort levels
  (low…max), shown as an active-AI label.
- **Background task queue** — reviews run in the background across tabs, FIFO with a
  concurrency cap; queued tasks are cancellable; live log tails; a Background Tasks
  tray.
- **Auto-triage (designed, currently manual — see §5)** — per-project policy for
  automatic triage on new/updated PRs; results land in the inbox.

### E. The Arena (multi-agent review tournament)

- **Concept** — run 2+ AI reviewers in parallel over multiple rounds, then an
  **arbiter** model reconciles disagreements into a curated "final truth." Solves
  single-model noise/hallucination.
- **Two modes** — **Models** (same prompt across different frontier models) or
  **Agents** (different review lenses, batched). 1 reviewer = "single review" (skips
  the overlay, imports straight to Review).
- **Lifecycle** — launcher (pick reviewers, scope, rounds 1–3, arbiter, effort;
  **live cost estimate + $25 cost guardrail**) → running panel (per-reviewer
  Thinking/Done/Failed, round bar, run-in-background pill, cancel) → results overlay.
- **Rounds** — Propose → Cross-check (keep/drop/escalate/lower/merge/flag) → Arbiter
  verdicts (confidence; auto-accept ≥0.75).
- **Visualizations** — **Bracket** (tournament columns), **Matrix** (findings ×
  reviewers vote grid), **Funnel** (Proposed→Cross-checked→Resolved→Final, with
  attrition), **Vote legend**, **Final Truth** (curated findings by severity +
  rationale), **Finding Detail** (full ballot history).
- **Accept into Review** — promote arena findings into the normal review.
  **History** — past runs persisted per branch, reopenable/deletable.

### F. Comments, questions & threads

- **Two thread types:** **GitHub comments** (cyan, for PR discussion, two-way synced)
  vs **Questions** (yellow, private notes, "Notes" tab). Line-level or hunk-level.
- **Single-level reply threads**, edit, delete (cascade), **resolve**.
- **Promote question → GitHub comment**.
- **Per-comment staleness** — comments dim/warn when their target line changes.

### G. GitHub / PR integration (via `gh`)

- **Open PRs** read-only (by project, URL paste, or owner/repo/number) — no
  working-tree checkout by default; optional checkout path.
- **Live PR status** — state, draft, checks (CI pass/fail/pending), review decision,
  mergeable, labels, reviewers, recent comments/reviews. Shown on the Branch card.
- **Comment sync** — pull from GitHub, push local comments (all / one thread / one
  reply).
- **Submit a review** — batched line comments + summary + **Approve / Request
  changes / Comment**, or a standalone PR decision, or a top-level PR comment.

### H. Inbox / notifications

- **Cross-project notification center** in the sidebar — collects events across all
  configured projects, not just the open one. Backed by a persisted, schema-versioned
  JSON store (`~/.config/er/inbox.json`), capped at 200 items, newest-first, atomic
  writes.
- **Teaser** in the rail (top 2, unread first) + full **popover** with **All / Unread
  / Read** tabs, per-project filter, mark-all-read, clear-read.
- **Transition-based** — the backend stores per-PR observed state (review decision,
  requested reviewers, state, head SHA, CI state) and only emits an item when
  something *changed* since the last poll, deduped by a `dedupe_key`. It's a genuine
  "what changed while I was away" feed, not re-notify-on-every-poll.
- **Item kinds** — AI: `ai_review_done/failed`, `ai_triage_done/failed`. GitHub:
  `pr_review_approved`, `pr_review_changes_requested`, `review_requested`,
  `pr_merged/closed`, `ci_failed` (CI fetched lazily, 10-min TTL, only on transition
  into red), throttled `github_refresh_failed`. Severity-colored icons.
- **Native OS notifications** fire once per item for AI/PR kinds or any
  warning/error severity (on macOS release builds, from "Easy Review").
- **Producers/cadence** — background AI task completion, plus a **PR-cache refresh
  loop every 10 min** (and a manual ↻). Click an item → detail modal → **Open
  target** (PR review tab or local branch).

### I. Embedded browser + UI annotations

- **Per-tab browser** — native child webview loading real `localhost` URLs (handles
  OAuth/cookies). Layouts: **hidden / split / fullscreen** (⌘B cycles, ⌘⇧B
  fullscreen), resizable split.
- **Dev-URL detection** — reads `package.json` scripts (vite/next/port) to find the
  dev server.
- **UI annotations** — **annotate mode**: click elements on the live page to leave
  positioned notes, capture screenshots, with DOM/selector context. Annotations
  **re-anchor** and mark stale on layout change. Surfaced in the Notes tab. (A visual
  review surface for the *rendered app*, parallel to code review.)

### J. Embedded terminal

- **Bottom drawer PTY** (⌘T / `` ` ``), one session per tab, rooted at the tab's
  repo. Resizable, persists open/closed; runs `claude`/`git` alongside review.

### K. Settings

- **General / Projects / Terminal tabs.** Theme picker (8 themes, live), AI
  provider/model/effort, feature flags (which diff scopes show), display options
  (line numbers/wrap/split/tab width), terminal config.
- **Per-project review settings** — auto-triage toggles + timing + max diff size, and
  **review-ignore globs** (paths excluded from AI review diffs).
- Global vs per-repo `.er-config.toml` precedence; live apply.

### L. Export & artifacts

- **Export review** to Markdown (copy / save / preview view, ⌘⇧E) and **export to
  agent** handoff file. Renders comments, questions, findings, and UI annotations.
- **Review revisions** — stored review snapshots are listable/readable; artifacts
  deletable.
- **Managed storage** — all sidecars live under app data per repo/branch/view bucket,
  shared with the TUI.

---

## 5. Notes for design / future planning

- **The snapshot contract is the design boundary.** Any new surface needs a field in
  `snapshot.rs` + `types.ts`. The UI is a pure function of the snapshot — good for
  designing, since every state is enumerable.
- **Three "main views" share the center column** (diff, agent-output, export-review),
  plus a full settings view and the browser pane. A redesign has freedom here.
- **The right rail is the review hub** (Branch / Review / Notes) and is the densest
  information surface — a natural focus for redesign.
- **Heavy async everywhere** — almost every meaningful action is a background task
  surfaced via polling/toasts/the task tray. Loading/queued/running/failed states are
  first-class and need design love.
- **Auto-triage is a designed-but-dormant pipeline.** Automatic dispatch on PR
  refresh is currently **disabled** in code — triage is kicked **manually** from the
  sidebar (`run_pr_triage` / `run_branch_triage`). The `auto_triage` flag and full
  gating policy (own-PRs, `new-only` / `review-requested` / `new-and-push`,
  max-diff-KB guard, ignore globs, skip-already-triaged-by-head-SHA) are retained for
  settings compatibility. Re-enabling the "auto-triage → inbox" loop is mostly a
  policy/UX decision, not new plumbing — a clean future-feature opportunity.
- **Observed gaps / future-leaning opportunities** (from roadmap docs + code): a true
  **review heatmap/coverage on exit**, **blame-aware findings**, **diff bookmarks**,
  and **richer human-override UI in the arena** are planned-or-thin. The inbox could
  grow into a real cross-project review queue; auto-triage → arena hand-off is a
  natural pipeline.

---

## 6. Source map (where to look)

| Area | Key files |
|------|-----------|
| App shell / layout | `desktop-ui/src/App.svelte` |
| Snapshot contract | `crates/er-desktop/src/snapshot.rs` ↔ `desktop-ui/src/lib/types.ts` |
| Command surface | `crates/er-desktop/src/commands.rs` |
| Keyboard routing | `desktop-ui/src/lib/stores/keyboard.ts` |
| Left sidebar / projects / inbox | `desktop-ui/src/lib/components/LeftSidebar.svelte` |
| Right rail (review hub) | `desktop-ui/src/lib/components/RightPanel.svelte` + `*Card.svelte` |
| Diff view | `desktop-ui/src/lib/components/DiffView.svelte`, `FlatDiffView.svelte`, `diff-rows/` |
| Arena | `desktop-ui/src/lib/arena/`, `components/arena/`, `crates/er-engine/src/arena/`, `crates/er-desktop/src/arena_commands.rs` |
| Inbox / auto-triage (backend) | `crates/er-desktop/src/inbox.rs`, `auto_triage.rs` |
| Browser + annotations | `desktop-ui/src/lib/components/BrowserView.svelte`, `AnnotationOverlay.svelte`; `crates/er-desktop/src/browser_*.rs`, `frame_script.rs` |
| Terminal | `desktop-ui/src/lib/components/Terminal.svelte`, `crates/er-desktop/src/terminal.rs` |
| Settings | `desktop-ui/src/lib/components/settings/`, `crates/er-desktop/src/config_commands.rs` |
| Export | `desktop-ui/src/lib/components/ExportReviewView.svelte`, `crates/er-desktop/src/export.rs` |
| Theming | `desktop-ui/src/app.css`, `desktop-ui/src/lib/themes.ts`, `syntaxThemes.ts` |
</content>
</invoke>
