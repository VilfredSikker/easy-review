# Epic 1.0.0 status

> **GitHub is the source of truth.** Track the work in the epic issue [#371](https://github.com/VilfredSikker/easy-review/issues/371), its sub-issues and the [v1.0.0 milestone](https://github.com/VilfredSikker/easy-review/milestone/1). Who has a task is its assignee. Its state is open or closed plus the linked PR, and what blocks it is GitHub's "blocked by" list. This file does not repeat that.
>
> Release branch `release/v1.0.0`. Plan: [PLAN.md](./PLAN.md). Design: [the proposal](../../plans/plan-clean-slate-architecture.md). Decisions: [ADDENDUM-01](./ADDENDUM-01-decisions.md), [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md), [ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md). Pick-up notes: [BUILD_LOG.md](./BUILD_LOG.md).

## Start here

Planning for phases 0–2 is closed (2026-11-06). Every task and design issue has a review verdict. Assign an issue when it has no open blocker and no assigned issue holds any of its lanes.

Hand the work out in bundles, with the prompts in [BUNDLES.md](./BUNDLES.md). Assignable at handoff:

| Issue | What | Model | Lanes |
| --- | --- | --- | --- |
| [#372](https://github.com/VilfredSikker/easy-review/issues/372) | P01: delete legacy feedback | sonnet | E, F |
| [#373](https://github.com/VilfredSikker/easy-review/issues/373) | P02: delete legacy config paths | opus | C |
| [#401](https://github.com/VilfredSikker/easy-review/issues/401) | P14 design: store envelope and keys | opus | - |
| [#402](https://github.com/VilfredSikker/easy-review/issues/402) | P20 design: findings projection | opus | - |
| [#403](https://github.com/VilfredSikker/easy-review/issues/403) | P05 design: no duplicate posts on retry | opus | - |
| [#407](https://github.com/VilfredSikker/easy-review/issues/407) | P07 design: `Git` trait and handle | opus | - |
| [#408](https://github.com/VilfredSikker/easy-review/issues/408) | P10 design: `GhError` classification | opus | - |
| [#409](https://github.com/VilfredSikker/easy-review/issues/409) | P10 design: cache, runner, default handle | opus | - |
| [#410](https://github.com/VilfredSikker/easy-review/issues/410) | P11 design: cache merge | opus | - |

Any model may take any of these. The `Model` column says how deep the Opus review goes, not who implements ([ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md)). When a PR is open or a design issue has its `**Decision:**` comment, bring it to the orchestrating session for review.

Design issues hold no lane and change no code, so all seven can run at once. Settling one means posting a `**Decision:**` comment and a `**Signatures:**` comment, then the owner approves and closes it ([ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md)). #403, #407 and #401 unblock the most work: P05, then P07 and the ports, then the store.

## Plan id to issue

The plan and the addenda refer to work by id. This table maps each id to its issue and does not change.

| Id | Issue |
| --- | --- |
| P01 | [#372](https://github.com/VilfredSikker/easy-review/issues/372) |
| P02 | [#373](https://github.com/VilfredSikker/easy-review/issues/373) |
| P03 | [#374](https://github.com/VilfredSikker/easy-review/issues/374) |
| P04 | [#375](https://github.com/VilfredSikker/easy-review/issues/375) |
| P05 | [#376](https://github.com/VilfredSikker/easy-review/issues/376) |
| P06 | [#377](https://github.com/VilfredSikker/easy-review/issues/377) |
| P07 | [#378](https://github.com/VilfredSikker/easy-review/issues/378) |
| P08 | [#379](https://github.com/VilfredSikker/easy-review/issues/379) |
| P09 | [#380](https://github.com/VilfredSikker/easy-review/issues/380) |
| P10 | [#381](https://github.com/VilfredSikker/easy-review/issues/381) |
| P11 | [#382](https://github.com/VilfredSikker/easy-review/issues/382) |
| P12 | [#383](https://github.com/VilfredSikker/easy-review/issues/383) |
| P13 | [#384](https://github.com/VilfredSikker/easy-review/issues/384) |
| P14 | [#385](https://github.com/VilfredSikker/easy-review/issues/385) |
| P15 | [#386](https://github.com/VilfredSikker/easy-review/issues/386) |
| P16 | [#387](https://github.com/VilfredSikker/easy-review/issues/387) |
| P17 | [#388](https://github.com/VilfredSikker/easy-review/issues/388) |
| P18 | [#389](https://github.com/VilfredSikker/easy-review/issues/389) |
| P19 | [#390](https://github.com/VilfredSikker/easy-review/issues/390) |
| P20 | [#391](https://github.com/VilfredSikker/easy-review/issues/391) |
| P21 | [#392](https://github.com/VilfredSikker/easy-review/issues/392) |
| P22 | [#393](https://github.com/VilfredSikker/easy-review/issues/393) |
| P23 | [#394](https://github.com/VilfredSikker/easy-review/issues/394) |
| P24 | [#395](https://github.com/VilfredSikker/easy-review/issues/395) |
| P25 | [#411](https://github.com/VilfredSikker/easy-review/issues/411) |
| G3 | [#396](https://github.com/VilfredSikker/easy-review/issues/396) |
| G4 | [#397](https://github.com/VilfredSikker/easy-review/issues/397) |
| G5 | [#398](https://github.com/VilfredSikker/easy-review/issues/398) |
| G5b | [#399](https://github.com/VilfredSikker/easy-review/issues/399) |
| G6 | [#400](https://github.com/VilfredSikker/easy-review/issues/400) |
| P05-design | [#403](https://github.com/VilfredSikker/easy-review/issues/403) |
| P07-design | [#407](https://github.com/VilfredSikker/easy-review/issues/407) |
| P10-design (errors) | [#408](https://github.com/VilfredSikker/easy-review/issues/408) |
| P10-design (cache, runner) | [#409](https://github.com/VilfredSikker/easy-review/issues/409) |
| P11-design | [#410](https://github.com/VilfredSikker/easy-review/issues/410) |
| P14-design | [#401](https://github.com/VilfredSikker/easy-review/issues/401) |
| P16-design | [#405](https://github.com/VilfredSikker/easy-review/issues/405) |
| P20-design | [#402](https://github.com/VilfredSikker/easy-review/issues/402) |
| P21-design | [#404](https://github.com/VilfredSikker/easy-review/issues/404) |
| P23-design | [#406](https://github.com/VilfredSikker/easy-review/issues/406) |

## Waiting on the owner

**Decisions.** D1–D10 are settled in [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md), and D11–D14 in [ADDENDUM-02-issue-decisions.md](./ADDENDUM-02-issue-decisions.md). None is open.

**Actions.**

1. Before P10 merges: provide real `gh` error samples (404, 401, 403 rate limit, 409, 422) if the implementer cannot produce them from a test repo.
