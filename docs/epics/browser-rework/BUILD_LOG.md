# Browser rework build log

Only the orchestrator updates this append-only log and [STATUS.md](./STATUS.md).

## How to resume

1. Read [PLAN.md](./PLAN.md), [AGENT_BRIEF.md](./AGENT_BRIEF.md), STATUS and the
   latest log entries. Fetch the current release branch before starting a lane.
2. Inspect open browser rework PRs against `release/v0.6.0` and their reviews.
   Resume an existing implementer when possible; do not duplicate a PR.
3. Use worktrees outside the main checkout. In this cloud session, source
   `/workspace/scratch/browser-tool-env.sh` before Rust, Bun or just commands.
4. Run heavy checks sequentially under the shared lock documented in the brief.
5. Native proof results must come from the real macOS application. Never promote
   a pending result to yes based on unit tests or a headless browser.
6. Update STATUS and append outcomes here after each reviewed integration.

## Log

### 2026-10-30

- The owner resumed implementation using the epic-orchestration skill. General
  work uses Terra; complex implementation and architectural judgment use Sol.
- Cloned the correct Easy Review repository. The current release was verified as
  `release/v0.6.0`, initially at `db43ec6c`; the unrelated Ink Booking checkout is
  untouched. The existing planning branch remains unmerged at `98f11917`.
- Original task and reference documents are available from the planning branch.
  The first execution slice is fixtures and engine-only v2 storage, followed by
  a renderer and MCP reader that do not remove the working v1 host flow.
- The runtime and host editor tasks must land in one combined PR. Live native
  integration remains gated on the macOS proof tasks.
- Installed a workspace-local Rust, Bun and just toolchain. Tool setup changed
  no application sources. Desktop system libraries have not been installed.
- Planning documents committed and pushed as `a51849b6`, after incorporating the
  concurrent merge of PR 324 at `3e32955e`. Its snapshot changes do not overlap the
  first engine/fixture slice; later snapshot work must use that updated base.
- Started ER21 and ER32a in separate worktrees under the delegated model policy.
  The shared toolchain and check lock are ready; no native proof has run.
- ER21 opened PR 330. Four Bun HTTP tests (84 assertions), redirect/host/port
  curl checks and docs-check passed. Independent review found the hostile
  synthetic-click fixture targeted the document rather than an element, weakening
  the later trust test; returned to the same implementer for a focused fix.
- Reviewed the focused ER21 synthetic-click fix at `83437941` and merged PR 330
  with a merge commit after the routing suite passed again. The proof log remains
  pending and the fixture's Safari DOM check remains an owner action.
- Corrected GNU/BSD stat fallback order in the installed shared-check helper so
  Linux waiters can read the lock age. The repository's locking policy is unchanged.
- ER32a PR 332 passed independent review and twelve storage tests, formatting,
  headless compilation and scoped engine Clippy. PR CI passed. Merged the reviewed
  head `d71588bd` as `1ba952af`; v1 callers remain unchanged.
- ER34a pure renderer PR 335 is under independent review. Eight renderer golden
  tests and fourteen existing export tests passed; no existing reader switched.
  ER35 implementation is stacked on that renderer and cannot ship ahead of it.
- Started ER32b against the reconciled PR 324 snapshot. Its read path remains
  separate from live draft/save integration.
- Started ER22 debug-only parking proof scaffolding. Its dedicated probe child
  avoids the product modal teardown confounder; this changes the experiment
  driver, not the release browser behavior. Mac compile and native observations
  remain required before the proof can pass.

### 2026-10-31

- The owner resumed the paused epic. Rechecked the current release branch, open
  PRs, saved worktrees, toolchain and existing reviews before resuming agents.
- Renderer PR 335 retained its reviewed head and passing CI. Merged it as
  `b22019ff`; this adds the renderer without switching production export readers.
- ER32b was rebased and checked locally. Independent review found a missing
  Debug derive, a cache-publication/sent-revision race and unrecoverable cached
  read errors. Assigned the repair to Sol before further publication.
- ER35 is locally committed at `ed37a5c4`; independent review found no blocker.
  Its 25 MCP tests, formatting and scoped Clippy passed, with no external writes.
- ER22 debug-only probe scaffolding is locally reviewed for a draft. macOS
  compilation and native behavior remain unverified; all 18 proof cells remain
  pending. The dedicated probe leaves product modal teardown unchanged.
- Automatic approval review rejected snapshot PR creation for public source
  disclosure. Verified the original repository is public, owned by VilfredSikker,
  and writable by the connected account; a supported retry with this evidence was
  also rejected for lack of explicit source-disclosure approval. Put remaining
  publication on hold while completing local checks and reviews.
