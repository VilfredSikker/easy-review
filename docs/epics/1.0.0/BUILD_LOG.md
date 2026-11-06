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
- The 24 task issues are too terse for a smaller model. An enrichment pass by another model is briefed in [HANDOFF-issue-enrichment.md](./HANDOFF-issue-enrichment.md), and its output is reviewed against [REVIEW-issue-enrichment.md](./REVIEW-issue-enrichment.md). Nine issues (P05, P07, P10, P11, P14, P16, P20, P21, P23) get a design issue that blocks them. Assign an issue only after a `ready` verdict.
- The enrichment pass is done. All 24 task issues follow the template in HANDOFF-issue-enrichment.md, and each has a `Plan correction` section where the code disagreed with the plan. Ten design issues block their implementation issues: #401 P14, #402 P20, #403 P05, #404 P21, #405 P16, #406 P23, #407 P07, #408 and #409 P10, #410 P11. Model labels moved to sonnet for #372, #378, #386 and #391. Nothing is reviewed yet.
- Decisions the review must settle before work starts:
  - `github.rs` has 19 production `git` spawns that no plan entry owns (verified). Either P08 (#379) migrates them, or they stay on the allow-list until P11 (#382).
  - #393 (P22) is stopped until its file-list correction is ruled on.
  - #384's `RepoSlug::parse` rule table is the agent's choice and needs confirming.
  - The P23 design (#406) must raise `rust-version` from 1.88 to 1.89 to use `File::lock`, or use `fs4` (noted on the issue).
- The owner settled the four open questions (D11–D14, [ADDENDUM-02-issue-decisions.md](./ADDENDUM-02-issue-decisions.md)):
  - P08 migrates `github.rs`'s git calls.
  - P22 takes the full app-records fix.
  - P13's behaviour changes are accepted.
  - Locks use `std::fs::File::lock`.
  The owner also asked for current dependencies, so P25 (#411, upgrade dependencies and raise `rust-version`) joins phase 0. It is blocked by #374 and blocks #378 and #394. Each affected issue has a "Settled" comment.
- Review pass done (four Opus reviewers, one per slice). Every task and design issue has a `**Review verdict:**` comment. 34 are `ready`; #376 (P05) is `design-first` until #403 is settled. Fixes were applied straight to the issue bodies, including:
  - #376's steps would have made local-only AI threads post to GitHub;
  - #379's production `App::new_empty` would have held a `FakeGit`;
  - #401's option B would have hidden every stale review, so the diff-hash check moved to ingest, in #395;
  - #387 missed the MCP reply and resolve writer;
  - a pathspec that matched nothing appeared in four issues.
  Lanes: #379 holds E and F; #381 holds C and E; #384 holds F; #387, #388, #390, #394 and #395 hold C.
- [ADDENDUM-03-issues-are-the-spec.md](./ADDENDUM-03-issues-are-the-spec.md) makes each issue body the specification and defines how design issues are settled. **Planning for phases 0–2 is closed.** STATUS.md "Start here" lists what is assignable at handoff.
- Owner decision: every task and design issue goes to smaller models. Opus reviews each PR and each design decision in the orchestrating session. `model:` labels now set review depth (ADDENDUM-03, "Who does the work, and who reviews it").
- Coordinator role handed to a new session; its prompt is [COORDINATOR.md](./COORDINATOR.md).

### 2026-11-06 (coordinator)

- Owner merged PR #365 and PR #368 into `main`. Merged `main` into `release/v1.0.0` (v0.6.5, #365, #368, #370; clean). Per D10, PROPOSAL.md is now a pointer to `docs/plans/plan-clean-slate-architecture.md`. `main` brought ADR 0052, so the next free ADR number is 0053.
- Design reviews posted on all seven wave-1 decisions (#401, #402, #403, #407, #408, #409, #410): each is `**Design review:** revise`. In every one the chosen option holds; the gaps are in signatures, the task issue changes, and the code that PR #370 brought in. Cross-design points: #409 and #410 both put a `PrCommentBundle` cache in #381, which cannot build it until #382; #403, #409 and #410 each need a cache generation counter against a fetch that is still running when the cache is cleared.
- Owner decision on #401 must-fix 4: the v1 store names branch and repo folders `<readable slug>-<8 hex of SHA-256 of the raw name>`; repos hash the normalized `host/owner/repo`. Applies from #386 (P15).
- Merged PR #412 (P01) as c3075f5fc and PR #413 (P02) as ccdca41af; #372 and #373 closed. T2 continues with #375 (P04), T1 with #374 (P03).
- Round-2 design reviews: #403 and #410 approve; #401, #402, #407, #408 and #409 revise. Owner decision on #401: the repo folder hash covers the lowercased `owner/repo` only, with no host (supersedes `host/owner/repo`).
- #403 approved and closed. Its revised signatures and five round-2 amendments are applied to the #376 and #377 bodies; #376 is `agent-ready` and waits only for #375. #410 is approved but is applied and closed together with #409.
- Owner delegation: the coordinator merges on a `merge` verdict with green CI and approves designs that pass review. The owner is asked only about important decisions (format or product trade-offs, anything irreversible or outside the plan, changes to D1–D14).
- Sessions are now messaged directly (SendMessage to the T and D sessions) to read their issue's latest `**Next step (coordinator):**` comment, and they message the coordinator after each GitHub post. GitHub stays the record.
- Merged PR #414 (P04) as 540072e67; #375 closed. The T2 session continues as T3 (#376, then #377).
- Designs approved by the coordinator under the owner's delegation, with their task issue changes applied: #402 (round 3) to #391; #407 (round 3) to #378, #379 and #380; #401 (round 3, with amendment B: `slug_repo` and `rules_key` stay unhashed, and only folder-path callers use the hashed slug) to #385, #386, #387, #393 and #395, with constraints recorded on #404 and #406. #401, #402 and #407 are closed.
- ADR numbers: 0053 goes to P05 (#376). Every other new ADR takes the next free number at branch time, because several PRs add ADRs in parallel.
- Design bundle D6 started: #404 to the D5 session and #406 to the D4 session. #405 waits for #381.
