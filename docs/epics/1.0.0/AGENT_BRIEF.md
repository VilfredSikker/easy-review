# Agent brief: epic 1.0.0

Every implementer and reviewer agent reads this first. Your GitHub issue is your specification ([ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md)): where it disagrees with [PLAN.md](./PLAN.md) or an earlier addendum, the issue wins, and a settled `**Decision:**` comment on its design issue wins over the issue. The target design is [the proposal](../../plans/plan-clean-slate-architecture.md).

## Ground rules

- **Your task is a GitHub issue.** Each PR has one issue under the epic [#371](https://github.com/VilfredSikker/easy-review/issues/371). The issue holds the scope, its "blocked by" links and its lane. Start only when every blocking issue is closed and no other assigned issue holds your lane. Assign yourself, or ask the owner to, before you begin.
- **Base and target.** All work branches from and targets `release/v1.0.0`, never `main`.
- **Branch names.** Use `epic/pNN-<slug>`, as named in the plan.
- **Repo rules.** Read the root [CLAUDE.md](../../../CLAUDE.md), [CONTEXT.md](../../../CONTEXT.md), [writing rules](../../agents/writing-docs.md) and [development guide](../../DEVELOPMENT.md), then the `CLAUDE.md` or `agent.md` in each directory you touch and the ADRs your PR cites. Use `CONTEXT.md` vocabulary for every new name.
- **Scope.** Do exactly your PR's scope from the plan. When you find something outside it, note it under "follow-ups" in your report instead of fixing it. If the plan is wrong about the code (a symbol moved, a caller exists that it says does not), stop and report with evidence and a recommendation.
- **Lint gate.** It only tightens (ADR 0038). Never grow `clippy-warning-budget.json` or a UI lint baseline. Fix the violation, or scope an `#[expect(lint, reason = …)]` / `// eslint-disable-next-line rule -- why` to the one site. After fixing a baselined violation, run `just clippy-prune` or `just lint-ui-prune`. List every baseline, allow-list or `clippy.toml` change under **Call-outs**.
- **Guardrails.** When your PR creates a crate or a port, it also adds the boundary rule the plan names for it. Do not leave it for later.
- **Old data.** Version 1.0.0 carries no old data. A deletion needs no compatibility read path. Do not add migrations unless the plan says so.
- **Tracking.** Report progress, blockers and plan corrections as comments on your issue. Do not edit `docs/epics/1.0.0/STATUS.md` or `BUILD_LOG.md`.
- **Model names.** Keep model names and agent ids out of code, comments, commits and PR text. Do not add attribution trailers to commits or PRs.
- **Comments.** Code says what and how; a comment says why. Do not cite plan ids, review findings or tracker numbers in code comments (`just comments-check` rejects them).

## Workspace

- **Worktree.** The orchestrator gives you a worktree path, normally `~/.claude-worktrees/er-v1/pNN`, outside the main checkout. Work only there. If it does not exist, create it:

  ```
  git -C /Users/vilfredsikker/Projects/easy-review fetch origin release/v1.0.0
  git -C /Users/vilfredsikker/Projects/easy-review worktree add -b epic/pNN-<slug> ~/.claude-worktrees/er-v1/pNN origin/release/v1.0.0
  ```

  For UI work, run `cd desktop-ui && bun install` in the worktree. Worktrees share build caches with nothing else; the first build is slow.
- **Shared machine.** Other agents run at the same time.
  - Run every heavy check (build, test, clippy, lint, format) through the machine-wide lock: `~/.claude/scripts/locked.sh <worktree> <command…>`. It serialises them across agents and sessions.
  - Go fastest first: the tests for files you changed, then the affected crate, then the full set below.
  - Never `pkill` or `killall` a pattern such as `cargo`, `node`, `bun` or `vitest`. Stop only processes you started, by PID.
  - A timeout that happens only under load, in a test that passes when run alone, is not a failure. Report the test names.
  - Do not start a dev server unless your PR needs one; reuse a running one (`lsof -ti tcp:<port>`).

## Verification before you push

Run each through `~/.claude/scripts/locked.sh <worktree>`:

- **Always:** `just fmt-check`, `just clippy`, `just test`, `just docs-check`, `just comments-check`
- **When `er-desktop` or `desktop-ui` is touched:** `just check`, `just lint-ui`, `just test-ui`, `just test-desktop`
- **When `er-mcp` is touched:** `just build-mcp`
- Plus the acceptance commands in your PLAN entry.

CI runs on PRs into `release/**` as well, so a red check on your PR is yours to fix.

Prove a fix or new behaviour with a test that fails before and passes after. Never skip, disable or delete a test to get green. If a failure already exists on `release/v1.0.0` and is unrelated to your PR, show that it fails there too and note it in your report.

## Commit, push, PR

1. **Commit.** Small logical commits with short messages that say why. Stage files by name; never `git add .`.
2. **Push** with `git push -u origin epic/pNN-<slug>`. On a network error, retry up to 4 times with backoff.
3. **Open the PR** with `gh pr create`.
   - Repo `VilfredSikker/easy-review`, base `release/v1.0.0`, title starting with `[Pnn]`, and `Part of #<issue>` as the first line of the body. A PR into a release branch does not close its issue; the person who merges closes it.
   - Body sections, short: **The Problem**, **How we fix**, **Design choices** (only when relevant), **How to manually test**. Then:
     - **Verification:** the exact commands and results, with test counts.
     - **Call-outs:** baseline, `clippy.toml`, `Cargo.toml` dependency or ADR changes.
     - **Owner actions:** anything only the owner can do.
     - **Follow-ups.**
4. **Do not merge.** The owner or orchestrator merges after review.

## Report back

Post this as a comment on your issue, and return it to whoever started you. Keep it to 250 words or fewer. Include:

- PR number and URL, branch, head SHA
- verification per command: pass or fail, with counts
- call-outs
- owner actions
- deviations from the plan, and why
- follow-ups
- anything the next PR in the lane must know

## Reviewers

1. **Read the diff:** `git diff origin/release/v1.0.0...origin/epic/pNN-<slug>`.
2. **Check it against the plan entry,** the proposal section it implements, and the repo rules.
3. **Look for:**
   - correctness bugs, especially in persistence, process control and gh writes
   - behaviour the PR removes that the plan did not ask to remove
   - missing tests, or tests that would pass against a broken implementation
   - scope creep
   - lint baseline growth or a guardrail the plan asked for and the PR skipped
   - rule violations and missing call-outs
4. **Verification:** re-run only what you doubt, through the lock.
5. **Post one PR review** with `gh pr review --comment`. List the findings by severity, each with a `file:line`.
6. **Report** the verdict as a comment on the issue, in 200 words or fewer.
   - **Verdict:** `merge`, `fix-then-merge` (with a list of must-fix items), or `rework`.
   - Include the findings.
