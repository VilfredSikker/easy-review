# Epic 1.0.0 status

> Release branch `release/v1.0.0`; every PR below targets it. Plan: [PLAN.md](./PLAN.md). Design: [the proposal](../../plans/plan-clean-slate-architecture.md). Decisions: [ADDENDUM-01](./ADDENDUM-01-decisions.md), [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md), [ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md). Pick-up notes: [BUILD_LOG.md](./BUILD_LOG.md).
> Updated by the coordinator only, after every merge, verdict or hand-off. GitHub ([#371](https://github.com/VilfredSikker/easy-review/issues/371), the [v1.0.0 milestone](https://github.com/VilfredSikker/easy-review/milestone/1)) wins if the two disagree. Last update: 2026-11-10 09:05 UTC.

**Legend:** ⬜ planned · 🟡 in progress · 🔵 in review · 🔧 fixing review findings · ✅ merged · ⛔ blocked on a design

## Now

| Session | Bundle | Working on | Lanes |
| --- | --- | --- | --- |
| T1 | T5 GitHub port | [#391](https://github.com/VilfredSikker/easy-review/issues/391) P20 | E |
| T2 | T4 git port | idle; next #441 after P19 and P20 | – |
| D4 | T6 store core | [#390](https://github.com/VilfredSikker/easy-review/issues/390) P19 | E |
| D1 | D6 | idle | – |
| D2, D3, D5 | – | idle | – |

**Phases 0 and 1 are complete; in phase 2, P14–P18, P22 and P23 are merged.** P19 (D4) and P20 (T1) run at the same time in the AI loaders; whichever merges second merges the release branch in. The project-list import (#453, T2) is in review. #441 waits for P19 and P20. Next: P20 unblocks P21, and P19 unblocks P24 (with P23 done).

## Progress

The `Model` column sets how deep the coordinator's review goes, not who implements ([ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md)). "After …" notes list the issue's open "blocked by" links on GitHub at the last update.

| PR | Title | Lane | Model | Status | PR link | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| P01 | [#372](https://github.com/VilfredSikker/easy-review/issues/372) Delete legacy feedback | E/F | sonnet | ✅ | [#412](https://github.com/VilfredSikker/easy-review/pull/412) | |
| P02 | [#373](https://github.com/VilfredSikker/easy-review/issues/373) Delete legacy config paths | C | opus | ✅ | [#413](https://github.com/VilfredSikker/easy-review/pull/413) | |
| P03 | [#374](https://github.com/VilfredSikker/easy-review/issues/374) Delete the importance table | C | opus | ✅ | [#415](https://github.com/VilfredSikker/easy-review/pull/415) | follow-up: ADR 0037 names removed types |
| P04 | [#375](https://github.com/VilfredSikker/easy-review/issues/375) Delete the repo-local storage root | E/F | opus | ✅ | [#414](https://github.com/VilfredSikker/easy-review/pull/414) | |
| P25 | [#411](https://github.com/VilfredSikker/easy-review/issues/411) Upgrade dependencies, raise rust-version | C | sonnet | ✅ | [#418](https://github.com/VilfredSikker/easy-review/pull/418) | TypeScript 7 and Tailwind 4.3.5 deferred |
| P07b | [#437](https://github.com/VilfredSikker/easy-review/issues/437) Spawn-rule escapes (cfg(not(test)), renamed import) | C | sonnet | ✅ | [#439](https://github.com/VilfredSikker/easy-review/pull/439) | P07 review follow-up |
| Flaky | [#438](https://github.com/VilfredSikker/easy-review/issues/438) Flaky dirty-diff desktop test | F | opus | ✅ | [#440](https://github.com/VilfredSikker/easy-review/pull/440) | cause: clock-named temp dirs; follow-up #441 |
| P13b | [#445](https://github.com/VilfredSikker/easy-review/issues/445) Storage slug through the shared parser | E | sonnet | ✅ | [#448](https://github.com/VilfredSikker/easy-review/pull/448) | Windows drive paths no longer read as remotes |
| Flaky-b | [#441](https://github.com/VilfredSikker/easy-review/issues/441) Env mutation and clock temp dirs in engine tests | E | sonnet | ⬜ | | held until P19, P20; scope adds the storage-root guard |
| P22b | [#452](https://github.com/VilfredSikker/easy-review/issues/452) Import the old project list once (D15) | E | sonnet | ✅ | [#453](https://github.com/VilfredSikker/easy-review/pull/453) | owner's 11 projects import in a copy |
| P0-gate | [#422](https://github.com/VilfredSikker/easy-review/issues/422) ADR 0037 wording, comments-check plan-id pattern | – | sonnet | ✅ | [#423](https://github.com/VilfredSikker/easy-review/pull/423) | |
| P05 | [#376](https://github.com/VilfredSikker/easy-review/issues/376) Post GitHub comments when created | E/F | opus | ✅ | [#416](https://github.com/VilfredSikker/easy-review/pull/416), [#417](https://github.com/VilfredSikker/easy-review/pull/417) | two stacked PRs |
| P06 | [#377](https://github.com/VilfredSikker/easy-review/issues/377) Delete the local review draft | E/F | opus | ✅ | [#420](https://github.com/VilfredSikker/easy-review/pull/420), [#421](https://github.com/VilfredSikker/easy-review/pull/421) | release note: old batch-review comments may show twice |
| P07 | [#378](https://github.com/VilfredSikker/easy-review/issues/378) Create er-git | C | sonnet | ✅ | [#419](https://github.com/VilfredSikker/easy-review/pull/419) | follow-ups in the PR review (cfg-test match, gh rule message) |
| P08 | [#379](https://github.com/VilfredSikker/easy-review/issues/379) Engine git calls behind the port | E/F | sonnet | ✅ | [#424](https://github.com/VilfredSikker/easy-review/pull/424), [#426](https://github.com/VilfredSikker/easy-review/pull/426) | two stacked PRs |
| P09 | [#380](https://github.com/VilfredSikker/easy-review/issues/380) Front-end git calls, git guardrail | C/F | sonnet | ✅ | [#428](https://github.com/VilfredSikker/easy-review/pull/428) | follow-up #432 merged |
| P10 | [#381](https://github.com/VilfredSikker/easy-review/issues/381) Create er-github | C/E | opus | ✅ | [#425](https://github.com/VilfredSikker/easy-review/pull/425), [#427](https://github.com/VilfredSikker/easy-review/pull/427) | two parts; real `gh` captures; all #408 rules pinned by planted breaks |
| P11 | [#382](https://github.com/VilfredSikker/easy-review/issues/382) Move github.rs into er-github | E | opus | ✅ | [#431](https://github.com/VilfredSikker/easy-review/pull/431), [#433](https://github.com/VilfredSikker/easy-review/pull/433) | two parts; P11 complete |
| P12 | [#383](https://github.com/VilfredSikker/easy-review/issues/383) Front-end gh calls, gh guardrail | C/F | opus | ✅ | [#446](https://github.com/VilfredSikker/easy-review/pull/446) | gh allow-list deleted |
| P13 | [#384](https://github.com/VilfredSikker/easy-review/issues/384) One remote-slug parser | F | sonnet | ✅ | [#442](https://github.com/VilfredSikker/easy-review/pull/442) | follow-up #445 |
| P14 | [#385](https://github.com/VilfredSikker/easy-review/issues/385) Create er-store | C | opus | ✅ | [#429](https://github.com/VilfredSikker/easy-review/pull/429), [#430](https://github.com/VilfredSikker/easy-review/pull/430) | two parts; amendment C on #401 |
| P15 | [#386](https://github.com/VilfredSikker/easy-review/issues/386) Move to the v1 root | C/E | sonnet | ✅ | [#435](https://github.com/VilfredSikker/easy-review/pull/435) | follow-up #436 merged |
| P16 | [#387](https://github.com/VilfredSikker/easy-review/issues/387) Comments and notes through the store | C/E/F | opus | ✅ | [#443](https://github.com/VilfredSikker/easy-review/pull/443), [#444](https://github.com/VilfredSikker/easy-review/pull/444) | two parts; follow-ups #441, #445 |
| P17 | [#388](https://github.com/VilfredSikker/easy-review/issues/388) Session state through the store | C/E | opus | ✅ | [#450](https://github.com/VilfredSikker/easy-review/pull/450) | reviewed save 2.9x accepted (5 ms budget) |
| P18 | [#389](https://github.com/VilfredSikker/easy-review/issues/389) Review artifacts through the store | E | opus | ✅ | [#454](https://github.com/VilfredSikker/easy-review/pull/454) | follow-ups carried into P19 |
| P19 | [#390](https://github.com/VilfredSikker/easy-review/issues/390) Remaining AI artifacts through the store | C/E | opus | 🔵 | [#456](https://github.com/VilfredSikker/easy-review/pull/456) | D4; in review |
| P20 | [#391](https://github.com/VilfredSikker/easy-review/issues/391) Findings projection | E | sonnet | 🔵 | [#457](https://github.com/VilfredSikker/easy-review/pull/457) | T1; in review; ADR 0058 |
| P21 | [#392](https://github.com/VilfredSikker/easy-review/issues/392) Scoped re-review as its own artifact | E/F | opus | ⬜ | | after P20 |
| P22 | [#393](https://github.com/VilfredSikker/easy-review/issues/393) App records through the store | F | opus | ✅ | [#449](https://github.com/VilfredSikker/easy-review/pull/449), [#451](https://github.com/VilfredSikker/easy-review/pull/451) | two PRs; the import is #452 |
| P23 | [#394](https://github.com/VilfredSikker/easy-review/issues/394) Cross-process locks, change notifications | C | opus | ✅ | [#434](https://github.com/VilfredSikker/easy-review/pull/434) | ADR 0057; Linux watcher fix proven on CI |
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
| [#405](https://github.com/VilfredSikker/easy-review/issues/405) P16: annotation types, promote boundary | P16 | ✅ | amendments A1–A8; applied to #387 |
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
