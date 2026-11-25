# Shared rules for epic 1.0.0 agents (cloud session)

(Copy of the cloud session's builder rules; recreate `/home/user/er-v1/` with this file and locked.sh, and fix the scratch path to the new session's scratchpad.)

Repo VilfredSikker/easy-review, release branch `release/v1.0.0`. Main checkout
`/home/user/easy-review` (do not edit it). Item work happens in git worktrees under
`/home/user/er-v1/<dir>`; never edit another agent's worktree.

## Spec
- `docs/epics/1.0.0/AGENT_BRIEF.md`, the plan entry for the item: phase 5b (TUI)
  entries P63–P77 are in `docs/epics/1.0.0/ADDENDUM-07-phase-5b.md` (read its
  decisions, lane T, the chain and the risks); phase 5 is ADDENDUM-06, now done.
  Then the item issue INCLUDING ALL COMMENTS.
- Phase 5 changed the tree after ADDENDUM-07 was written: er-desktop has no App
  and no `v1-service` feature; `commands.rs` → `route_helpers.rs` and
  `snapshot.rs` → `page_view.rs`; the agent launcher, `card_job` and
  `card_ai_context` moved to er-engine's always-compiled half. The App and
  er-tui's `v1-service` feature remain for the TUI until P76/P77. Re-verify
  every citation by symbol before building. Where they differ from the
  addendum, the latest **Coordinator ruling** comment wins.
- Read `CLAUDE.md`, `CONTEXT.md` and the ADRs touching the area.

## Tools in this container
- No working `gh` CLI. Use the GitHub MCP tools (`mcp__github__*`; load them
  first with ToolSearch, e.g. `select:mcp__github__issue_read,mcp__github__pull_request_read,mcp__github__add_issue_comment,mcp__github__create_pull_request,mcp__github__update_pull_request`).
  Owner `VilfredSikker`, repo `easy-review`.
- `just`, cargo 1.98.0, bun (`/root/.bun/bin`) and the Tauri Linux libraries are installed.
- desktop-ui gates need `desktop-ui/node_modules`: run `bun install --frozen-lockfile` in the worktree's `desktop-ui` once.
- `git fetch origin <branch>` and `git push origin <branch>` work. Commits are
  signed automatically (SSH key in git config); do not change git config. If a
  commit ever fails to sign, say so in the PR body.

## Checks (keep them cheap: disk is limited)
- GitHub CI runs the full suites (cargo test, test-v1, protocol, clippy, fmt,
  headless, desktop-ui) on every PR push, and the coordinator merges only on
  green CI. So do NOT run `just test`, `test-desktop`, `test-v1`, `test-all`,
  `build-*` or release builds locally. Read CI results with
  `mcp__github__pull_request_read` (method `get_check_runs`) after pushing.
- Local checks, through the lock, foreground, with a timeout:
  `timeout 3600 /home/user/er-v1/locked.sh /home/user/er-v1/<dir> <cmd>`.
  Cheap ones: `just fmt-check docs-check comments-check lint-ui check-ui test-ui`
  (bun, no cargo) and `cargo check --workspace --all-targets` (no codegen);
  `cargo check -p er-tui --all-targets --features v1-service` for the TUI flag path,
  `just check-desktop` when er-desktop or er-engine features change.
  `just clippy` only once per PR, at the end.
- A PR whose base is not `release/v1.0.0` gets no CI. For such a stacked PR,
  run the full test suite of every crate you changed in lane S (usually
  `cargo test -p er-service`) once through the lock before handing back, and
  compare failures with the base branch (some fail only as root here).
- Planted breaks are the one place a test must run locally: run ONLY the test(s)
  concerned, e.g. `cargo test -p er-tui --features v1-service <test_name>`.
  The lock sets debuginfo and incremental off and gives each worktree its own target dir under /home/user/er-v1/targets/; delete yours (rm -rf /home/user/er-v1/targets/<dir>) when you remove the worktree.
- `check-ui` has one known error in `src/main.ts`. In this container `test-ui`
  has 26 environment failures that also fail on release: compare the failing set
  with release's, not the count. Known flaky test
  `projects::tests::a_pin_landing_during_a_desktop_update_survives` (#630): name it if it fails, never change it.
- Never read or write the owner's real app data (`~/Library/Application Support/easy-review`, `~/.local/share/easy-review`).
  If locked.sh prints a WARNING about `easy-review/v1/app`, report it.
- Write scratch files only under `/tmp/claude-0/-home-user-easy-review/97ee0ed2-5125-5c87-81ee-e31b6e1bdf04/scratchpad/<dir>/`.

## Tests and plants
- Every test added for a done-when item must fail against a planted break.
  COMMIT FIRST, then apply and revert each plant (`git checkout -- <file>`);
  paste the failing output in the PR. Never leave a plant in the tree.
- Never weaken or skip a test to go green. State any change to an existing
  test up front in the PR body ("Test change, up front").

## Stop line
1,500 counted lines per PR: insertions of `git diff --shortstat -M <base>...<head>`.
No waivers. Over it: split into stacked PRs, or trim comments only.

## Git
- Short commit messages that say why; never `git add .`; never rebase, never
  force-push; a pushed branch takes merges only (merge `origin/release/v1.0.0`
  or the parent branch in). On conflicts take the union of enums, match arms
  and test lists, and regenerate the generated TypeScript with
  `just protocol-gen` instead of hand-merging it.
- NO attribution, co-author or session lines in commits, PR bodies or comments.
  No "Generated by" footer. The GitHub tools add one to a PR body on create:
  remove it with mcp__github__update_pull_request. On issue/PR comments the
  session proxy re-adds it and it cannot be removed; leave those.

- Never push an empty commit to start CI. If CI does not run on a PR, tell the
  coordinator; the coordinator retargets and closes/reopens.

## PR
- Title starts with `[<item id>]`. Body sections: The Problem, How we fix,
  Design choices (if relevant), How to manually test; plus "Part of #<issue>",
  the plant outputs, the gates run with results, and the stop-line measurement.

## Prose
Plain. Never use the words "load-bearing", "mint(ed)" or "rung(s)", nor the
"not X, but Y" sentence shape.

## Limits
- Do NOT merge any PR. The coordinator merges.
- If a plan premise is false, or a decision outside the approved plan is
  needed, stop and report it with your recommended option (you may post the
  question on the item issue). Do not decide product questions yourself.
- Final reply: compact, findings first. Builders: status, PRs (number, branch,
  base, head SHA, counted lines), gates, plants, notes. Reviewers: verdict
  (merge / fix-then-merge / block), numbered must-fixes with evidence, notes.
