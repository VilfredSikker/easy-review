# Epic 1.0.0 status

> Release branch `release/v1.0.0`; every PR below targets it. Plan: [PLAN.md](./PLAN.md). Design: [the proposal](../../plans/plan-clean-slate-architecture.md). Decisions: [ADDENDUM-01](./ADDENDUM-01-decisions.md), [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md), [ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md), [ADDENDUM-04](./ADDENDUM-04-phase-3.md). Pick-up notes: [BUILD_LOG.md](./BUILD_LOG.md).
> Updated by the coordinator only, after every merge, verdict or hand-off. GitHub ([#371](https://github.com/VilfredSikker/easy-review/issues/371), the [v1.0.0 milestone](https://github.com/VilfredSikker/easy-review/milestone/1)) wins if the two disagree. Last update: 2026-11-10 18:24 UTC.

**Legend:** ⬜ planned · 🟡 in progress · 🔵 in review · 🔧 fixing review findings · ✅ merged · ⛔ blocked on a design

## Now

| Session | Bundle | Working on | Lanes |
| --- | --- | --- | --- |
| D3 | – | idle | – |
| D1 | D6 | [#474](https://github.com/VilfredSikker/easy-review/issues/474) P27, step plan | E, F |
| D4 | T6 store core | [#464](https://github.com/VilfredSikker/easy-review/issues/464) store-lock fix | E, W |
| T2 | T4 git port | [#493](https://github.com/VilfredSikker/easy-review/issues/493) agent_spawn matcher gaps | C |
| T1, D2, D5 | – | idle | – |

**Phases 0–2 are complete, and the phase-3 gate passed on 2026-11-10.** [ADDENDUM-04](./ADDENDUM-04-phase-3.md) is the phase-3 plan, and its issues are #471–#489. P35, P26a and P26b are merged; P27 is next; P30a waits for #464.

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
| Flaky-b | [#441](https://github.com/VilfredSikker/easy-review/issues/441) Env mutation and clock temp dirs in engine tests | E | sonnet | ✅ | [#461](https://github.com/VilfredSikker/easy-review/pull/461), [#462](https://github.com/VilfredSikker/easy-review/pull/462) | T2; done. Follow-up [#465](https://github.com/VilfredSikker/easy-review/issues/465) merged in [#466](https://github.com/VilfredSikker/easy-review/pull/466) |
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
| P19 | [#390](https://github.com/VilfredSikker/easy-review/issues/390) Remaining AI artifacts through the store | C/E | opus | ✅ | [#456](https://github.com/VilfredSikker/easy-review/pull/456) | closes the P18 follow-ups |
| P20 | [#391](https://github.com/VilfredSikker/easy-review/issues/391) Findings projection | E | sonnet | ✅ | [#457](https://github.com/VilfredSikker/easy-review/pull/457) | ADR 0058 |
| P21 | [#392](https://github.com/VilfredSikker/easy-review/issues/392) Scoped re-review as its own artifact | E/F | opus | ✅ | [#463](https://github.com/VilfredSikker/easy-review/pull/463) | D4; done after three rounds. Follow-up [#464](https://github.com/VilfredSikker/easy-review/issues/464) (D4) |
| P22 | [#393](https://github.com/VilfredSikker/easy-review/issues/393) App records through the store | F | opus | ✅ | [#449](https://github.com/VilfredSikker/easy-review/pull/449), [#451](https://github.com/VilfredSikker/easy-review/pull/451) | two PRs; the import is #452 |
| P23 | [#394](https://github.com/VilfredSikker/easy-review/issues/394) Cross-process locks, change notifications | C | opus | ✅ | [#434](https://github.com/VilfredSikker/easy-review/pull/434) | ADR 0057; Linux watcher fix proven on CI |
| P24 | [#395](https://github.com/VilfredSikker/easy-review/issues/395) MCP uploads through the store | C | opus | ✅ | [#458](https://github.com/VilfredSikker/easy-review/pull/458) | schemas derived; every release path pinned |
| Lock | [#464](https://github.com/VilfredSikker/easy-review/issues/464) Sidecar writers outside the store lock | E/W | opus | 🔵 | [#491](https://github.com/VilfredSikker/easy-review/pull/491) | D4; round 2 in review |
| Flaky-c | [#468](https://github.com/VilfredSikker/easy-review/issues/468) ETXTBSY when tests write and run a script | E | sonnet | ✅ | [#469](https://github.com/VilfredSikker/easy-review/pull/469) | T2; Linux plant proven on draft #470 |
| P35 | [#471](https://github.com/VilfredSikker/easy-review/issues/471) D1: delete the browser annotation modules | E | sonnet | ✅ | [#490](https://github.com/VilfredSikker/easy-review/pull/490) | D3 |
| P26a | [#472](https://github.com/VilfredSikker/easy-review/issues/472) Create er-agent (runtime and the guardrail) | C/E/F | opus | ✅ | [#492](https://github.com/VilfredSikker/easy-review/pull/492) | D1; follow-up [#493](https://github.com/VilfredSikker/easy-review/issues/493) |
| Spawn-gaps | [#493](https://github.com/VilfredSikker/easy-review/issues/493) agent_spawn matcher gaps from the P26a review | C/F | sonnet | 🟡 | | T2; before P33b |
| P26b | [#473](https://github.com/VilfredSikker/easy-review/issues/473) ProviderSpec, one decoder, JobSpec/JobRunner | C/E | opus | ✅ | [#494](https://github.com/VilfredSikker/easy-review/pull/494) | D1 |
| P27 | [#474](https://github.com/VilfredSikker/easy-review/issues/474) Card AI and model discovery onto the Provider | E/F | opus | 🟡 | | D1; step plan |
| P28a | [#475](https://github.com/VilfredSikker/easy-review/issues/475) Background queue onto the JobRunner | E/W | opus | ⬜ | | after #474 |
| P28b | [#476](https://github.com/VilfredSikker/easy-review/issues/476) Review-flow launch and the Deep-run cancel/orphan test | W | opus | ⬜ | | after #475 |
| P29 | [#477](https://github.com/VilfredSikker/easy-review/issues/477) Tab-local prompts, the shell path and the summary job (D18) | E | opus | ⬜ | | after #476 |
| P30a | [#478](https://github.com/VilfredSikker/easy-review/issues/478) Host commits job outputs | E/C | opus | ⬜ | | after #475, #477, #464 |
| P32a | [#479](https://github.com/VilfredSikker/easy-review/issues/479) Workflow record and crash resume | W | opus | ⬜ | | after #476 |
| P32b | [#480](https://github.com/VilfredSikker/easy-review/issues/480) Panel stage and the free round measurement (D17) | W | opus | ⬜ | | after #479 |
| P33a | [#481](https://github.com/VilfredSikker/easy-review/issues/481) Arena reviewers and arbiter onto the JobRunner | W | opus | ⬜ | | after #480 |
| P33b | [#482](https://github.com/VilfredSikker/easy-review/issues/482) Delete the arena registry, pid kill, `progress.jsonl` and glue | W | opus | ⬜ | | after #477, #481 |
| P30b.1 | [#483](https://github.com/VilfredSikker/easy-review/issues/483) Port the remaining raw readers/writers onto the store | E/W/C | opus | ⬜ | | after #478, #482 |
| P30b.2 | [#484](https://github.com/VilfredSikker/easy-review/issues/484) Envelope every agent kind; delete `Mode::Bare` | E/C | opus | ⬜ | | after #483 |
| P31 | [#485](https://github.com/VilfredSikker/easy-review/issues/485) Importance as a RiskRules artifact | C/E | opus | ⬜ | | after #484 |
| P37 | [#486](https://github.com/VilfredSikker/easy-review/issues/486) Split the agent config into `agents.toml` (D16, A7) | C/E/F | opus | ⬜ | | after #485 |
| P34a | [#487](https://github.com/VilfredSikker/easy-review/issues/487) Accepted findings and the projection | W/E | opus | ⬜ | | after #482, #486 |
| P34b | [#488](https://github.com/VilfredSikker/easy-review/issues/488) Delete the arena import, the rollback and the provenance run read | W/E | opus | ⬜ | | after #487 |
| P36 | [#489](https://github.com/VilfredSikker/easy-review/issues/489) Registry gate | E/C/F | opus | ⬜ | | after #486 |

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
| Phase 3: jobs and workflows | [#396](https://github.com/VilfredSikker/easy-review/issues/396) | ✅ passed 2026-11-10; [ADDENDUM-04](./ADDENDUM-04-phase-3.md), issues #471–#489 |
| Phase 4: core and service | [#397](https://github.com/VilfredSikker/easy-review/issues/397) | ⬜ |
| Phase 5: desktop protocol | [#398](https://github.com/VilfredSikker/easy-review/issues/398) | ⬜ |
| Phase 5b: TUI | [#399](https://github.com/VilfredSikker/easy-review/issues/399) | ⬜ |
| Phase 6: consolidate | [#400](https://github.com/VilfredSikker/easy-review/issues/400) | ⬜ |

## Waiting on the owner

**Decisions.** D1–D10 are settled in [ADDENDUM-01](./ADDENDUM-01-decisions.md) and D11–D18 in [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md). Routine merges and design approvals are delegated to the coordinator (BUILD_LOG, 2026-11-06). None is open.

**Actions.**

None open. The real 409 and 422 `gh` captures are taken (in `~/er-gh-fixtures/`, recorded on #381); the 403 rate limit stays synthetic.
