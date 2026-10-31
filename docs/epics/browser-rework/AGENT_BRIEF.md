# Browser rework agent brief

Read [PLAN.md](./PLAN.md), your PR entry and its original T-task via the pinned
git-show commands before editing. An owner ADDENDUM overrides the plan. Read root
[AGENTS.md](../../../AGENTS.md), [CLAUDE.md](../../../CLAUDE.md),
[CONTEXT.md](../../../CONTEXT.md), [writing rules](../../agents/writing-docs.md),
[development guide](../../DEVELOPMENT.md), then local agent instructions and ADRs.

## Work boundaries

- Base and PR target are `release/v0.6.0`; branch names are exactly those in PLAN.
  The old planning branch is a source, not an implementation base.
- The orchestrator commits planning docs before dispatch and assigns a worktree
  outside the checkout, normally `/workspace/easy-review-worktrees/er21` or its
  assigned PR ID. Work only there; do not edit another agent's checkout.
- Only the orchestrator edits STATUS and BUILD_LOG. Report outcomes to it.
- Preserve current product behavior in the first slice: fixtures, pure storage,
  pure renderer and MCP reader do not activate new browser runtime or remove v1
  readers. ER32b avoids the snapshot lane until PR 324 is reconciled.
- Native proof scaffolding is debug-only and pending real macOS evidence.
  Do not implement dependent identity/runtime/presentation before passed gates.
  ER30+31 must merge together in one PR.
- Keep engine code synchronous and UI-agnostic; support headless consumers.
  Keep disk/native work outside the App mutex, view buckets stable, revision
  delivery push-driven and configuration global. Read ADR 0004, ADR 0015,
  ADR 0034 and ADR 0038 when relevant. No lint baseline growth.
- Do not invent answers to the open contract decisions in PLAN. Source typos and
  stale line numbers can be reconciled against current symbols; material scope
  or behavior conflicts go to the orchestrator with evidence and a recommendation.
- No automatic agent messages or external publishing. Follow the orchestrator's
  explicit commit/push/PR authorization; never merge your own work.

## Machine and verification

Use existing toolchains. Consult the development guide before adding dependencies;
avoid repository-wide reformatting or lockfile churn. Never stop another agent's
process. Start fixture servers with recorded PIDs and stop only those PIDs.

Heavy checks are serial across all worktrees. Use the installed shared wrapper,
which supersedes the historical Claude-home path in original task files:

```bash
LOCK=/home/agent/.codex/skills/confidence-merge/scripts/locked.sh
REPO=$(git rev-parse --show-toplevel)
"$LOCK" "$REPO" ./scripts/er-tui.sh test -p er-engine browser_annotations
"$LOCK" "$REPO" just docs-check
```

Select your PLAN commands, then relevant lint gates. Report exact commands,
results, test counts and missing prerequisites. Never weaken/delete tests, grow a
baseline, claim an unrun suite passed, or treat a Linux UI simulation as native
proof. Run full suites only when required or a specific unresolved risk warrants
them. The orchestrator coordinates shared caches and toolchain changes.

## Delivery and review

Keep each patch reviewable; flag approaching 1,500 changed lines before expanding
scope. Report branch/head SHA, files changed, verification, owner actions, open
decisions, deviations and follow-ups in at most 250 words. Include proof state
separately from scaffold state. Keep model names and task references out of code
comments and public PR prose; they belong in coordination docs.

When authorized to open a PR, target the release branch, use the PLAN ID in its
title and explain behavior, validation, call-outs, owner actions and follow-ups.
Use small logical commits and the session's requested attribution, if any.
Reviewers inspect the current diff against PLAN and repo rules, focusing on
trust boundaries, durable writes, stale callbacks and behavior regression. Return
`merge`, `fix-then-merge` or `rework` with concrete file/line findings. A GitHub
review, when authorized, uses COMMENT; only the orchestrator merges the reviewed
head. A native-dependent PR is not ready solely because its debug code compiles.
