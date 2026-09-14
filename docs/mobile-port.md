# Mobile port — complexity assessment

**Status:** Draft (planning)  
**Last updated:** 2026-09-13

---

## Scope (proposed)

No tracked branches, no local git workflows. Mobile easy-review covers:

- PR list: my PRs and PRs to review
- Open PR → diff review
- GitHub comments (pull and sync; push if the write path lands)
- Personal questions and notes
- AI review artifacts (read, and possibly run — see [AI review](#3-ai-review-read-vs-run))

Everything else follows desktop remote-PR behavior.

---

## Scope lands on an existing path

Both front ends already review a remote PR with no clone: a PR tab reads the PR's own refs and never moves the working tree (ADR 0020), and its sidecars go to the PR bucket of managed storage (ADR 0003, ADR 0004). Mobile is that path productized, not a new review model.

---

## What transfers, what drops

Transfers inside `er-engine`: unified diff parsing with compaction and lazy per-file parsing; comment anchor resolution (hunk content match) and merge; the sidecar set with staleness by diff hash; the snapshot DTOs the desktop frontend consumes (ADR 0006).

Dropping tracked branches removes whole subsystems: file watch, staging / commit / history, worktrees, open-in-editor, watched files, conflict mode, branch tracking.

Two things do not transfer. GitHub I/O is `gh` subprocesses (below). The frontend is Svelte + Tauri; the concepts carry — flat rows, virtual scroll, viewport lazy loading, highlighting on the consumer side (ADR 0014) — the stack does not.

---

## Major complexity areas (ordered by impact)

### 1. GitHub access: `gh` is not portable (highest)

Every GitHub operation in `er-engine` spawns `gh`: PR diff, metadata, checks, review threads, comment create/reply/delete, review submission. ADR 0026 records why, and that a hosted front end can share the engine only up to this boundary.

Mobile must:

- Run an OAuth device flow and store the token in Keychain / Keystore
- Reimplement each operation over REST or GraphQL, with pagination and error shapes the CLI currently hides
- Read rate-limit headers and gate its own call volume
- Pick a policy for oversized PRs. Today they are refused by a size guard (`REMOTE_PR_MAX_*`), with a shallow clone as the large-PR fallback; neither is viable on device.

Remote-tab logic needs no change. The transport layer is new surface area, and the largest single piece.

### 2. Where `er-engine` runs: three viable architectures

| Approach | Pros | Cons |
|----------|------|------|
| **A. Cloud API** (Rust service hosting the engine) | Maximum parity with desktop; keeps diff parse, anchors, sidecar rules; AI can run server-side | Hosting, auth, latency, privacy/compliance |
| **B. Rust core on device** (UniFFI / FFI into iOS/Android) | Offline-friendly; no server | Heavy build pipeline; still no `gh` or agent CLI on device |
| **C. Native app + reimplemented logic** | Best platform UX | Duplicates diff parse, sync and AI loading — drift risk |

**Pragmatic path:** start with **A** for parity and speed.

`er-api` is a deferred extraction, not an existing crate: `sync.rs` and `er-engine`'s Cargo comment name it as a headless consumer, and nothing implements it. The contract worth porting is the engine's own state model plus the snapshot and revision-event protocol (ADR 0006, ADR 0011, ADR 0012); mobile would be another consumer of it over HTTP/WebSocket instead of Tauri IPC.

### 3. AI review: read vs run (product decision)

**Read AI findings** — same sidecars desktop reads, staleness by diff hash.

**Run AI review on mobile** — hard, because the desktop path spawns a local agent CLI that writes sidecars with shell and filesystem access. Neither iOS nor Android can assume a user-installed Claude Code, and the shipped review prompts assume a shell and a repo layout (ADR 0023). An on-device model call needs a second prompt path with the diff inlined.

Options, cheapest first:

- **Read-only mobile** — review runs on desktop or CI; mobile consumes results
- **Cloud-run review** — backend runs the same prompts; mobile triggers a job and polls. Mirrors the desktop's background review queue and process-wide slot cap (ADR 0021)
- **On-device model API** — HTTP to a model provider; needs the second prompt path

### 4. Storage and sync (medium)

Managed storage root is `$ER_STORAGE_ROOT`, or the platform app-data directory plus `easy-review`. A PR's artifacts live in `<storage_root>/repos/<owner-repo>/prs/pr-<N>/`; the repo `.er/` is debug-only (`ER_REPO_LOCAL=1`), and the one-time migration out of it never ran (ADR 0003).

Mobile needs:

- A per-PR sidecar store in the app sandbox: questions, notes, comments, review output, reviewed state
- A cross-device story when the same PR is reviewed on desktop and phone — conflict resolution keyed on `diff_hash` and comment ids
- Backup is not in the codebase today

Comment sync keys on tab identity (`tab_key`) so a background pull cannot race the active tab. Mobile inherits the same problem across screens.

### 5. Diff UI on small screens (medium–high UX)

Reuse the concepts, not the code: touch targets that separate line comments from hunk comments, keyboard-less navigation (file drawer, swipe between files), native or lightweight highlighting with a per-file budget, virtualization for large diffs. Split diff, blame and heatmap are v2 on desktop; defer them on mobile.

### 6. Freshness without a daemon (medium)

Desktop is push-driven: the backend emits a revision event and the frontend polls in response, with a 30s timer only as a safety net for events fired before the listener attaches (ADR 0011). Freshness of GitHub state comes from per-loop TTLs instead — a PR-head probe throttled to roughly a minute per PR, comment sync on a 45s cadence.

Mobile has no always-on process:

- Foreground refresh on resume, plus push notifications (PR updated, review requested)
- iOS background execution limits
- Offline: cached diff behind a stale banner, the same staleness UX as desktop

### 7. Cut or defer for v1

- Browser panel and UI annotations
- Embedded terminal
- Open-in-editor and worktree checkout — replace with an "Open in GitHub" deep link
- Commit composer, staging, watch mode, export-to-agent
- Multi-worktree tabs, tracked branches, local-agent inbox

Worth keeping if the API supports it: submit review (approve / comment) and push comments.

---

## Suggested phased delivery

**Phase 1 — read-heavy reviewer (lowest risk)**  
PR list → open PR → virtualized diff → pull GitHub comments → local questions → show AI findings if present. OAuth and REST diff. No on-device AI generation.

**Phase 2 — write path**  
Add, reply, and delete comments; push; resolve threads; submit a review decision. Refresh on resume.

**Phase 3 — AI parity**  
Cloud job runner for the review prompts, or sidecar sync from desktop; optional "Run review" button against the backend.

**Phase 4 — polish**  
Saved PRs, notifications, multiple accounts, conflict-aware sidecar sync across devices.

---

## Open decisions (affect cost 2–5x)

1. **Platform:** native vs cross-platform. Cross-platform can reuse the frontend's TypeScript types but still needs a native diff list for performance.
2. **AI on mobile:** read-only vs cloud-run vs on-device model API. Read-only avoids the subprocess and prompt problems entirely.
3. **Backend:** thin client speaking only to GitHub vs a hosted engine. A thin client duplicates anchor and parse logic; a backend keeps one source of truth.
4. **Privacy:** whether PR diffs and comments pass through your server (compliance for employer repos).

---

## Bottom line

Supporting easy-review on mobile is feasible and aligned with the existing remote-PR path, but it is not a small port of the desktop app. The scope removes the hardest local-git features; the work is:

1. Replacing **`gh`** with token-based GitHub API access
2. Choosing **where the engine and the AI run** (a hosted engine recommended for review generation)
3. Building a **touch-first diff and comment UI** with the same virtualized, lazy patterns as desktop
4. Defining **sidecar sync** between desktop and mobile for the same PR

Estimated relative effort (rough): GitHub API layer 35% · mobile diff/comment UI 30% · storage/sync 15% · AI strategy 15% · auth/accounts/notifications 5%.

---

## Planning checklist

- [ ] Decide native vs cross-platform, and thin GitHub client vs hosted engine API
- [ ] Decide the AI strategy: read-only sidecars vs cloud-run review vs on-device model API
- [ ] Design the GitHub REST/GraphQL module that replaces the `gh` subprocesses — see [github-sync-architecture.md](./github-sync-architecture.md) (deferred design reference) and ADR 0026
- [ ] Spec the snapshot and revision-event contract as the mobile API, drawn from the engine's state model (ADR 0006, ADR 0011)
- [ ] Spec the touch diff UI: virtualized hunks, lazy files, inline threads and findings
- [ ] Define per-PR sandbox paths, and optional cross-device sync for questions, comments and review output
- [ ] Record v1 exclusions: watch, branches, editor, browser, terminal, commit
