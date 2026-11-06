# Epic 1.0.0 build log

Append-only, newest at the bottom. Only the orchestrator writes here. For where things stand now, see [STATUS.md](./STATUS.md).

## How to resume (read first)

1. **Get the latest state.** GitHub is the source of truth: read epic issue #371 and its sub-issues (assignees, open or closed, blocked-by), then the last entries below. List the open PRs whose base is `release/v1.0.0`.
2. **Worktrees.** The epic docs are edited in `~/.claude-worktrees/er-v1/epic-docs` (branch `release/v1.0.0`). Each PR is built in `~/.claude-worktrees/er-v1/<id>`, outside the main checkout. Run `cd desktop-ui && bun install` in a worktree before touching the UI.
3. **Delegation** follows the `epic-orchestration` skill: model per the issue's `model:` label, a reviewer per PR, and merge only on a `merge` verdict.
4. **Phase gates.** Only phases 0–2 are planned as PRs. Before starting phase 3, re-plan it against the merged tree and record the new entries in an addendum.

## Log

### 2026-11-03

- Source: the clean-slate architecture proposal, committed as [PROPOSAL.md](./PROPOSAL.md). Its text is the same as `docs/plans/plan-clean-slate-architecture.md` in PR #365 (branch `claude/loving-ritchie-clz88g`, open against `main`). Once #365 merges and `main` is merged into `release/v1.0.0`, replace PROPOSAL.md with a link to that file so there is one copy.
- Decisions (from the owner, recorded in the proposal): the desktop leads and the TUI follows; the service runs in-process; one desktop window; keep the arena as a workflow stage; GitHub pending review is the only review draft; no migration of old data.
- Release branch `release/v1.0.0` created from `origin/main` at `604c7945d` (v0.6.4 merged; `release/v0.6.4` had nothing ahead of `main`). Named with the `v` prefix so the release-branch resolver in `CLAUDE.md` finds it.
- Side effect: the resolver in `CLAUDE.md` now returns `release/v1.0.0`, so other sessions doing feature work will target the breaking branch. Raised as an owner decision in STATUS.
- Planning: phases 0–2 detailed as PRs; phases 3–6 kept as milestones with exit criteria.
- PLAN.md drafted: 24 PRs over phases 0–2 (P01–P24), milestones for phases 3–6, owner decisions D1–D9 (D10 added for PR #365). Checked against the tree before commit: `release/v0.6.1` has 13 commits not on `main` (browser v2 callers, PRs 352 and 353), and the TUI posts to GitHub only through `push_comments_as_review` / `push_all_comments_to_github`, so deleting the local draft needs P05 first.
- No implementers started. Waiting on the owner for D1–D10, or for a go-ahead on the defaults.

### 2026-11-04

- Owner chose D1 (defer browser annotations to the phase 2 gate), D2 (keep v0.6.x on `release/v0.6.5`) and D3 (TUI review submission waits for 5b). The owner delegated D4–D10; the choices and reasons are in [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md).
- D7 changed from the plan's default: the spawn guardrails land at error tier with a shrink-only file allow-list, so `clippy-warning-budget.json` stays `{}`.
- D8 was based on a wrong premise: `chore/remove-dead-agent-runtime` merged as PR #227 into `release/v0.4.19`, and `agent_runtime.rs` was restored later. No action.
- D2: `release/v0.6.5` already existed (cut by the owner on 2026-11-03). The resolver change is PR #368 against `main`; until it merges, the resolver on `main` still returns `release/v1.0.0`.

### 2026-11-06

- The work moved to GitHub issues, which are now the source of truth. The epic is #371, with 24 PR issues (#372–#395) and 5 phase-gate issues (#396–#400), all sub-issues of #371 in the `v1.0.0` milestone. Native "blocked by" links follow the plan's dependencies. The labels are `epic:1.0.0`, `phase:*`, `lane:C/E/F`, `model:opus/sonnet`, `phase-gate` and `agent-ready`.
- STATUS.md now holds only the map from plan id to issue, plus the owner actions. The progress table was removed so there is one tracker.
- Before the issues were created, PLAN.md P07, P08, P09, P10, P12 and P15 were edited to match Addendum 01 (D7 allow-list, D6 notice).
- Agents claim an issue by assignment, report on the issue, and open PRs with `Part of #<issue>`. Whoever merges closes the issue, because PRs into a release branch do not close issues.
