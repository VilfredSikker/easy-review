# Epic 1.0.0 status

> Release branch `release/v1.0.0`; every PR below targets it. Plan: [PLAN.md](./PLAN.md). Design: [the proposal](../../plans/plan-clean-slate-architecture.md). Decisions: [ADDENDUM-01](./ADDENDUM-01-decisions.md), [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md), [ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md). Pick-up notes: [BUILD_LOG.md](./BUILD_LOG.md).
> Updated by the coordinator only, after every merge, verdict or hand-off. GitHub ([#371](https://github.com/VilfredSikker/easy-review/issues/371), the [v1.0.0 milestone](https://github.com/VilfredSikker/easy-review/milestone/1)) wins if the two disagree. Last update: 2026-11-06 21:20 UTC.

**Legend:** ⬜ planned · 🟡 in progress · 🔵 in review · 🔧 fixing review findings · ✅ merged · ⛔ blocked on a design

## Now

| Session | Bundle | Working on | Lanes |
| --- | --- | --- | --- |
| T1 → T4 | T4 git port, then T5 | reading [#381](https://github.com/VilfredSikker/easy-review/issues/381) P10; starts when #377 merges; #379 and #380 after #377 | – |
| T2 → T3 | T3 comment posting | [#377](https://github.com/VilfredSikker/easy-review/issues/377) P06 | E, F |
| D4 | – | idle; designs #401 and #406 done | – |
| D5 | – | idle; designs #402 and #404 done | – |
| D1, D2, D3 | – | idle; next free design is #405 when #381 closes | – |

**Next to unblock:** #377 (P06) merging closes phase 0 and starts #381 (P10) and #379 (P08). #377 and #381 touch the same files (#381 moves `github.rs` and `pending_review.rs`), so they run in that order.

## Progress

The `Model` column sets how deep the coordinator's review goes, not who implements ([ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md)). "After …" notes list the issue's open "blocked by" links on GitHub at the last update.

| PR | Title | Lane | Model | Status | PR link | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| P01 | [#372](https://github.com/VilfredSikker/easy-review/issues/372) Delete legacy feedback | E/F | sonnet | ✅ | [#412](https://github.com/VilfredSikker/easy-review/pull/412) | |
| P02 | [#373](https://github.com/VilfredSikker/easy-review/issues/373) Delete legacy config paths | C | opus | ✅ | [#413](https://github.com/VilfredSikker/easy-review/pull/413) | |
| P03 | [#374](https://github.com/VilfredSikker/easy-review/issues/374) Delete the importance table | C | opus | ✅ | [#415](https://github.com/VilfredSikker/easy-review/pull/415) | follow-up: ADR 0037 names removed types |
| P04 | [#375](https://github.com/VilfredSikker/easy-review/issues/375) Delete the repo-local storage root | E/F | opus | ✅ | [#414](https://github.com/VilfredSikker/easy-review/pull/414) | |
| P25 | [#411](https://github.com/VilfredSikker/easy-review/issues/411) Upgrade dependencies, raise rust-version | C | sonnet | ✅ | [#418](https://github.com/VilfredSikker/easy-review/pull/418) | TypeScript 7 and Tailwind 4.3.5 deferred |
| P05 | [#376](https://github.com/VilfredSikker/easy-review/issues/376) Post GitHub comments when created | E/F | opus | ✅ | [#416](https://github.com/VilfredSikker/easy-review/pull/416), [#417](https://github.com/VilfredSikker/easy-review/pull/417) | two stacked PRs |
| P06 | [#377](https://github.com/VilfredSikker/easy-review/issues/377) Delete the local review draft | E/F | opus | 🟡 | | T3; also the leftover `preload.rs` comment |
| P07 | [#378](https://github.com/VilfredSikker/easy-review/issues/378) Create er-git | C | sonnet | ✅ | [#419](https://github.com/VilfredSikker/easy-review/pull/419) | follow-ups in the PR review (cfg-test match, gh rule message) |
| P08 | [#379](https://github.com/VilfredSikker/easy-review/issues/379) Engine git calls behind the port | E/F | sonnet | ⬜ | | after P06 |
| P09 | [#380](https://github.com/VilfredSikker/easy-review/issues/380) Front-end git calls, git guardrail | C/F | sonnet | ⬜ | | after P08 |
| P10 | [#381](https://github.com/VilfredSikker/easy-review/issues/381) Create er-github | C/E | opus | ⬜ | | after P06 (shared files; coordinator order); T4 next; needs real `gh` samples (owner) |
| P11 | [#382](https://github.com/VilfredSikker/easy-review/issues/382) Move github.rs into er-github | E | opus | ⬜ | | after P10, P08 |
| P12 | [#383](https://github.com/VilfredSikker/easy-review/issues/383) Front-end gh calls, gh guardrail | C/F | opus | ⬜ | | after P11, P09 |
| P13 | [#384](https://github.com/VilfredSikker/easy-review/issues/384) One remote-slug parser | F | sonnet | ⬜ | | after P11 |
| P14 | [#385](https://github.com/VilfredSikker/easy-review/issues/385) Create er-store | C | opus | ⬜ | | after P10 |
| P15 | [#386](https://github.com/VilfredSikker/easy-review/issues/386) Move to the v1 root | C/E | sonnet | ⬜ | | after P14 |
| P16 | [#387](https://github.com/VilfredSikker/easy-review/issues/387) Comments and notes through the store | C/E/F | opus | ⛔ | | design #405; after P23, P15, P11, P06 |
| P17 | [#388](https://github.com/VilfredSikker/easy-review/issues/388) Session state through the store | C/E | opus | ⬜ | | after P16 |
| P18 | [#389](https://github.com/VilfredSikker/easy-review/issues/389) Review artifacts through the store | E | opus | ⬜ | | after P17 |
| P19 | [#390](https://github.com/VilfredSikker/easy-review/issues/390) Remaining AI artifacts through the store | C/E | opus | ⬜ | | after P18 |
| P20 | [#391](https://github.com/VilfredSikker/easy-review/issues/391) Findings projection | E | sonnet | ⬜ | | after P18 |
| P21 | [#392](https://github.com/VilfredSikker/easy-review/issues/392) Scoped re-review as its own artifact | E/F | opus | ⬜ | | after P20 |
| P22 | [#393](https://github.com/VilfredSikker/easy-review/issues/393) App records through the store | F | opus | ⬜ | | after P13, P15 |
| P23 | [#394](https://github.com/VilfredSikker/easy-review/issues/394) Cross-process locks, change notifications | C | opus | ⬜ | | after P25, P14 |
| P24 | [#395](https://github.com/VilfredSikker/easy-review/issues/395) MCP uploads through the store | C | opus | ⬜ | | after P19, P23 |

## Designs

| Design | Blocks | Status | Notes |
| --- | --- | --- | --- |
| [#403](https://github.com/VilfredSikker/easy-review/issues/403) P05: no duplicate posts on retry | P05 | ✅ | applied to #376, #377 |
| [#407](https://github.com/VilfredSikker/easy-review/issues/407) P07: `Git` trait and handle | P07 | ✅ | applied to #378–#380 |
| [#408](https://github.com/VilfredSikker/easy-review/issues/408) P10: `GhError` classification | P10 | ✅ | two binding amendments |
| [#409](https://github.com/VilfredSikker/easy-review/issues/409) P10: cache, runner, default handle | P10 | ✅ | |
| [#410](https://github.com/VilfredSikker/easy-review/issues/410) P11: cache merge | P11 | ✅ | applied to #382 |
| [#401](https://github.com/VilfredSikker/easy-review/issues/401) P14: store envelope and keys | P14 | ✅ | amendment B; applied to #385–#387, #393, #395 |
| [#402](https://github.com/VilfredSikker/easy-review/issues/402) P20: findings projection | P20 | ✅ | applied to #391 |
| [#404](https://github.com/VilfredSikker/easy-review/issues/404) P21: validation and scoped overlays | P21 | ✅ | amendments A1–A2; applied to #392 (plus the `z` cleanup step) and #391 |
| [#405](https://github.com/VilfredSikker/easy-review/issues/405) P16: annotation types, promote boundary | P16 | ⬜ | after #381 |
| [#406](https://github.com/VilfredSikker/easy-review/issues/406) P23: lock, lease, change channel | P23 | ✅ | amendments A1–A2; applied to #394 and #387–#389; P16 now follows P23 |

## Phase gates

| Gate | Issue | Status |
| --- | --- | --- |
| Phase 3: jobs and workflows | [#396](https://github.com/VilfredSikker/easy-review/issues/396) | ⬜ planned when phase 2 is merged |
| Phase 4: core and service | [#397](https://github.com/VilfredSikker/easy-review/issues/397) | ⬜ |
| Phase 5: desktop protocol | [#398](https://github.com/VilfredSikker/easy-review/issues/398) | ⬜ |
| Phase 5b: TUI | [#399](https://github.com/VilfredSikker/easy-review/issues/399) | ⬜ |
| Phase 6: consolidate | [#400](https://github.com/VilfredSikker/easy-review/issues/400) | ⬜ |

## Waiting on the owner

**Decisions.** D1–D10 are settled in [ADDENDUM-01](./ADDENDUM-01-decisions.md) and D11–D14 in [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md). Routine merges and design approvals are delegated to the coordinator (BUILD_LOG, 2026-11-06). None is open.

**Actions.**

None open. The real 409 and 422 `gh` captures are taken (in `~/er-gh-fixtures/`, recorded on #381); the 403 rate limit stays synthetic.
