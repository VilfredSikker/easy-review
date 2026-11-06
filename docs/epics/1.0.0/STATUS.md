# Epic 1.0.0 status

> **GitHub is the source of truth.** Track the work in the epic issue [#371](https://github.com/VilfredSikker/easy-review/issues/371), its sub-issues and the [v1.0.0 milestone](https://github.com/VilfredSikker/easy-review/milestone/1). Who has a task is its assignee. Its state is open or closed plus the linked PR, and what blocks it is GitHub's "blocked by" list. This file does not repeat that.
>
> Release branch `release/v1.0.0`. Plan: [PLAN.md](./PLAN.md). Design: [PROPOSAL.md](./PROPOSAL.md). Decisions: [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md). Pick-up notes: [BUILD_LOG.md](./BUILD_LOG.md).

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
| G3 | [#396](https://github.com/VilfredSikker/easy-review/issues/396) |
| G4 | [#397](https://github.com/VilfredSikker/easy-review/issues/397) |
| G5 | [#398](https://github.com/VilfredSikker/easy-review/issues/398) |
| G5b | [#399](https://github.com/VilfredSikker/easy-review/issues/399) |
| G6 | [#400](https://github.com/VilfredSikker/easy-review/issues/400) |

## Waiting on the owner

**Decisions.** D1–D10 are settled in [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md). None is open.

**Actions.**

1. Go-ahead to merge PR #365 into `main` (D10).
2. Merge PR #368 into `main`, so other sessions resolve to `release/v0.6.5` (D2).
3. Before P10 merges: provide real `gh` error samples (404, 401, 403 rate limit, 409, 422) if the implementer cannot produce them from a test repo.
