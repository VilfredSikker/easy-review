# Epic 1.0.0 status

> Release branch `release/v1.0.0`; every PR below targets it. Plan: [PLAN.md](./PLAN.md). Design: [the proposal](../../plans/plan-clean-slate-architecture.md). Decisions: [ADDENDUM-01](./ADDENDUM-01-decisions.md), [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md), [ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md), [ADDENDUM-04](./ADDENDUM-04-phase-3.md), [ADDENDUM-05](./ADDENDUM-05-phase-4.md). Pick-up notes: [BUILD_LOG.md](./BUILD_LOG.md).
> Updated by the coordinator only, after every merge, verdict or hand-off. GitHub ([#371](https://github.com/VilfredSikker/easy-review/issues/371), the [v1.0.0 milestone](https://github.com/VilfredSikker/easy-review/milestone/1)) wins if the two disagree. Last update: 2026-11-19 19:07 UTC.

**Legend:** ⬜ planned · 🟡 in progress · 🔵 in review · 🔧 fixing review findings · ✅ merged · ⛔ blocked on a design

## Now

| Session | Bundle | Working on | Lanes |
| --- | --- | --- | --- |
| Coordinator | workflow runs | Phase 5 ([#398](https://github.com/VilfredSikker/easy-review/issues/398)): P50–P52.2, P53.1 and P55.1 merged, and `main` merged in with the `head_oid` fix (#639); P53.2 is merging release in; P54 in review, stacked on P53.2; P55.2a has merge verdicts and waits for P53.2; the phase-5b plan is a draft in review (#640). The owner approved stacking P54 on P53.2, splitting P55.2, and planning 5b early | – |
| D1–D5, T1, T2 | – | stopped 2026-11-14; the coordinator runs the rest of phase 4 through workflow runs | – |

**Phases 0–2 are complete, and the phase-3 gate passed on 2026-11-10.** [ADDENDUM-04](./ADDENDUM-04-phase-3.md) is the phase-3 plan, and its issues are #471–#489. **Phase 3 is complete, and the phase-4 gate passed on 2026-11-14.** [ADDENDUM-05](./ADDENDUM-05-phase-4.md) is the phase-4 plan, re-checked at `afd78bced`; its issues are #526–#538. **Phase 4 is complete (2026-11-17):** P38 through P47.2 are merged and #526–#538 are closed. P47.2 was re-sequenced into stacked layers (2a, 2b, a slim 2c); its decisions, the exit checklist and the list of what stays on the App with the flag on are on #538. The phase-5 gate questions (D9 codegen, sidebar resources) are answered. **The phase-5 gate passed on 2026-11-18:** the owner approved the plan, [ADDENDUM-06](./ADDENDUM-06-phase-5.md) (merged as #601), and its issues are #602–#622.

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
| Lock | [#464](https://github.com/VilfredSikker/easy-review/issues/464) Sidecar writers outside the store lock | E/W | opus | ✅ | [#491](https://github.com/VilfredSikker/easy-review/pull/491) | D4; two rounds |
| Flaky-c | [#468](https://github.com/VilfredSikker/easy-review/issues/468) ETXTBSY when tests write and run a script | E | sonnet | ✅ | [#469](https://github.com/VilfredSikker/easy-review/pull/469) | T2; Linux plant proven on draft #470 |
| P35 | [#471](https://github.com/VilfredSikker/easy-review/issues/471) D1: delete the browser annotation modules | E | sonnet | ✅ | [#490](https://github.com/VilfredSikker/easy-review/pull/490) | D3 |
| P26a | [#472](https://github.com/VilfredSikker/easy-review/issues/472) Create er-agent (runtime and the guardrail) | C/E/F | opus | ✅ | [#492](https://github.com/VilfredSikker/easy-review/pull/492) | D1; follow-up [#493](https://github.com/VilfredSikker/easy-review/issues/493) |
| Spawn-gaps | [#493](https://github.com/VilfredSikker/easy-review/issues/493) agent_spawn matcher gaps from the P26a review | C/F | sonnet | ✅ | [#495](https://github.com/VilfredSikker/easy-review/pull/495) | T2 |
| P26b | [#473](https://github.com/VilfredSikker/easy-review/issues/473) ProviderSpec, one decoder, JobSpec/JobRunner | C/E | opus | ✅ | [#494](https://github.com/VilfredSikker/easy-review/pull/494) | D1 |
| P27 | [#474](https://github.com/VilfredSikker/easy-review/issues/474) Card AI and model discovery onto the Provider | E/F | opus | ✅ | [#496](https://github.com/VilfredSikker/easy-review/pull/496) | D1 |
| P28a | [#475](https://github.com/VilfredSikker/easy-review/issues/475) Background queue onto the JobRunner | E/W | opus | ✅ | [#497](https://github.com/VilfredSikker/easy-review/pull/497) | D1 |
| P28b | [#476](https://github.com/VilfredSikker/easy-review/issues/476) Review-flow launch and the Deep-run cancel/orphan test | W | opus | ✅ | [#498](https://github.com/VilfredSikker/easy-review/pull/498) | D1; three rounds, found and fixed an arena registry leak |
| P29 | [#477](https://github.com/VilfredSikker/easy-review/issues/477) Tab-local prompts, the shell path and the summary job (D18) | E | opus | ✅ | [#499](https://github.com/VilfredSikker/easy-review/pull/499) | D3 |
| P48 | [#502](https://github.com/VilfredSikker/easy-review/issues/502) Retire the legacy auto-triage flag (D22) | E/F | sonnet | ✅ | [#503](https://github.com/VilfredSikker/easy-review/pull/503) | T2 |
| P30a | [#478](https://github.com/VilfredSikker/easy-review/issues/478) Host commits job outputs | E/C | opus | ✅ | [#504](https://github.com/VilfredSikker/easy-review/pull/504), [#505](https://github.com/VilfredSikker/easy-review/pull/505) | D1; T2 reviewed both |
| P32a | [#479](https://github.com/VilfredSikker/easy-review/issues/479) Workflow record and crash resume | W | opus | ✅ | [#500](https://github.com/VilfredSikker/easy-review/pull/500), [#501](https://github.com/VilfredSikker/easy-review/pull/501) | D4; D3 reviewed |
| P32b | [#480](https://github.com/VilfredSikker/easy-review/issues/480) Panel stage and the free round measurement (D17) | W | opus | ✅ | [#508](https://github.com/VilfredSikker/easy-review/pull/508), [#509](https://github.com/VilfredSikker/easy-review/pull/509) | D4; D3 reviewed |
| P33a | [#481](https://github.com/VilfredSikker/easy-review/issues/481) Arena reviewers and arbiter onto the JobRunner | W | opus | ✅ | [#512](https://github.com/VilfredSikker/easy-review/pull/512) | D1; D4 and T2 reviewed |
| P33b | [#482](https://github.com/VilfredSikker/easy-review/issues/482) Delete the arena registry, pid kill, `progress.jsonl` and glue | W | opus | ✅ | [#513](https://github.com/VilfredSikker/easy-review/pull/513) | D4; D1 reviewed |
| P30b.1 | [#483](https://github.com/VilfredSikker/easy-review/issues/483) Port the remaining raw readers/writers onto the store | E/W/C | opus | ✅ | [#514](https://github.com/VilfredSikker/easy-review/pull/514), [#515](https://github.com/VilfredSikker/easy-review/pull/515) | D3; D1 and T2 reviewed; #514 stop line waived by the owner |
| P30b.2 | [#484](https://github.com/VilfredSikker/easy-review/issues/484) Envelope every agent kind; delete `Mode::Bare` | E/C | opus | ✅ | [#517](https://github.com/VilfredSikker/easy-review/pull/517), [#518](https://github.com/VilfredSikker/easy-review/pull/518) | D3; D1 and T2 reviewed |
| P31 | [#485](https://github.com/VilfredSikker/easy-review/issues/485) Importance as a RiskRules artifact | C/E | opus | ✅ | [#519](https://github.com/VilfredSikker/easy-review/pull/519) | D4; D1 reviewed |
| P37 | [#486](https://github.com/VilfredSikker/easy-review/issues/486) Split the agent config into `agents.toml` (D16, A7) | C/E/F | opus | ✅ | [#520](https://github.com/VilfredSikker/easy-review/pull/520) | D4; D1 reviewed |
| P34a | [#487](https://github.com/VilfredSikker/easy-review/issues/487) Accepted findings and the projection | W/E | opus | ✅ | [#521](https://github.com/VilfredSikker/easy-review/pull/521) | D3; T2 reviewed |
| P34b | [#488](https://github.com/VilfredSikker/easy-review/issues/488) Delete the arena import, the rollback and the provenance run read | W/E | opus | ✅ | [#523](https://github.com/VilfredSikker/easy-review/pull/523) | D3; D1 reviewed |
| P36 | [#489](https://github.com/VilfredSikker/easy-review/issues/489) Registry gate | E/C/F | opus | ✅ | [#524](https://github.com/VilfredSikker/easy-review/pull/524) | D3; T2 reviewed |
| D17 display | [#525](https://github.com/VilfredSikker/easy-review/issues/525) Show the D17 panel measurement in the desktop arena detail | F | opus | ✅ | [#539](https://github.com/VilfredSikker/easy-review/pull/539) | D1; coordinator reviewed |
| P38 | [#526](https://github.com/VilfredSikker/easy-review/issues/526) Build flag and the `er-service` skeleton | C/F | opus | ✅ | [#541](https://github.com/VilfredSikker/easy-review/pull/541) | D4; T2 reviewed |
| P39.1 | [#527](https://github.com/VilfredSikker/easy-review/issues/527) `er-core`: crate, types and the `Artifact` move | C/E-core | opus | ✅ | [#543](https://github.com/VilfredSikker/easy-review/pull/543), [#544](https://github.com/VilfredSikker/easy-review/pull/544) | D3; D1 reviewed |
| P39.2 | [#528](https://github.com/VilfredSikker/easy-review/issues/528) `er-core`: risk evaluation, hashes, branch helpers and arena rules | C/E-core/W | opus | ✅ | [#545](https://github.com/VilfredSikker/easy-review/pull/545) | D3; D1 reviewed |
| P39.3 | [#529](https://github.com/VilfredSikker/easy-review/issues/529) `er-core`: the projection | C/E-core | opus | ✅ | [#547](https://github.com/VilfredSikker/easy-review/pull/547) | D3; T2 reviewed |
| P40 | [#530](https://github.com/VilfredSikker/easy-review/issues/530) Workspace actor | E-svc | opus | ✅ | [#546](https://github.com/VilfredSikker/easy-review/pull/546) | T2; D4 reviewed |
| P41 | [#531](https://github.com/VilfredSikker/easy-review/issues/531) Annotation commands and the override deletion | E-svc/F | opus | ✅ | [#549](https://github.com/VilfredSikker/easy-review/pull/549), [#550](https://github.com/VilfredSikker/easy-review/pull/550), [#551](https://github.com/VilfredSikker/easy-review/pull/551), [#552](https://github.com/VilfredSikker/easy-review/pull/552) | D4, then workflow runs; reviewed |
| P42 | [#532](https://github.com/VilfredSikker/easy-review/issues/532) Sync and GitHub commands | E-svc/F | opus | ✅ | [#553](https://github.com/VilfredSikker/easy-review/pull/553), [#555](https://github.com/VilfredSikker/easy-review/pull/555) | workflow runs; reviewed |
| P43 | [#533](https://github.com/VilfredSikker/easy-review/issues/533) Job and workflow commands | W/E-svc/F | opus | ✅ | [#557](https://github.com/VilfredSikker/easy-review/pull/557), [#558](https://github.com/VilfredSikker/easy-review/pull/558), [#559](https://github.com/VilfredSikker/easy-review/pull/559), [#562](https://github.com/VilfredSikker/easy-review/pull/562), [#563](https://github.com/VilfredSikker/easy-review/pull/563), [#564](https://github.com/VilfredSikker/easy-review/pull/564), [#565](https://github.com/VilfredSikker/easy-review/pull/565) | P43.1 job sink and arena; P43.2 one review-flow driver (option A) |
| P44 | [#534](https://github.com/VilfredSikker/easy-review/issues/534) Settings schema | E-svc/F | opus | ✅ | [#556](https://github.com/VilfredSikker/easy-review/pull/556), [#560](https://github.com/VilfredSikker/easy-review/pull/560), [#561](https://github.com/VilfredSikker/easy-review/pull/561), [#568](https://github.com/VilfredSikker/easy-review/pull/568) | ADR 0069; P44d routes the AI picker saves |
| P45 | [#535](https://github.com/VilfredSikker/easy-review/issues/535) Tab persistence and open-target caches | E-svc/F | opus | ✅ | [#566](https://github.com/VilfredSikker/easy-review/pull/566), [#567](https://github.com/VilfredSikker/easy-review/pull/567) | ADR 0070; option B (open map for desktop fields) |
| P46 | [#536](https://github.com/VilfredSikker/easy-review/issues/536) Single-owner view state into the front ends | E-svc/F | opus | ✅ | [#554](https://github.com/VilfredSikker/easy-review/pull/554) | ADR 0068; stable process-local TabKey |
| P47.1 | [#537](https://github.com/VilfredSikker/easy-review/issues/537) Snapshot builder over the read API | F/E-svc | opus | ✅ | [#569](https://github.com/VilfredSikker/easy-review/pull/569), [#571](https://github.com/VilfredSikker/easy-review/pull/571) | re-sequenced (owner): the read seam and pure navigation; parity moved to P47.2 |
| P47.2 | [#538](https://github.com/VilfredSikker/easy-review/issues/538) Routing the remaining commands and the phase exit | F/E-svc/C | opus | ✅ | 2a: [#570](https://github.com/VilfredSikker/easy-review/pull/570), [#572](https://github.com/VilfredSikker/easy-review/pull/572), [#573](https://github.com/VilfredSikker/easy-review/pull/573), [#574](https://github.com/VilfredSikker/easy-review/pull/574), [#576](https://github.com/VilfredSikker/easy-review/pull/576); 2b: [#577](https://github.com/VilfredSikker/easy-review/pull/577), [#578](https://github.com/VilfredSikker/easy-review/pull/578); 2c.1: [#595](https://github.com/VilfredSikker/easy-review/pull/595), [#596](https://github.com/VilfredSikker/easy-review/pull/596), [#597](https://github.com/VilfredSikker/easy-review/pull/597); 2c.2: [#599](https://github.com/VilfredSikker/easy-review/pull/599) | 2a the Workspace owns tab content (ADR 0071); 2b one TabKey; 2c slim (owner): service-side gaps, then parity and the exit. CI plant: draft #598. Exit checklist and the "left on the App" list are in #538's body |
| P50 | [#602](https://github.com/VilfredSikker/easy-review/issues/602) er-protocol, ts-rs codegen and the drift check | C/P | opus | ✅ | [#624](https://github.com/VilfredSikker/easy-review/pull/624) | merged `9f62bf70e` |
| P51 | [#603](https://github.com/VilfredSikker/easy-review/issues/603) er::command_shape with a shrinking allow-list | C | opus | ✅ | [#626](https://github.com/VilfredSikker/easy-review/pull/626) | merged `cb62e2e19` |
| P52.1 | [#604](https://github.com/VilfredSikker/easy-review/issues/604) The resource host, the three commands and the backend tick | D/S/P/U/C | opus | ✅ | [#627](https://github.com/VilfredSikker/easy-review/pull/627), [#628](https://github.com/VilfredSikker/easy-review/pull/628), [#629](https://github.com/VilfredSikker/easy-review/pull/629) | merged `b31ca1e98` |
| P52.2 | [#605](https://github.com/VilfredSikker/easy-review/issues/605) The frontend resource client, the adapter pattern and the protocol ADR | U/P | opus | ✅ | [#631](https://github.com/VilfredSikker/easy-review/pull/631) | merged `5c22d5fe6` |
| P53 | [#606](https://github.com/VilfredSikker/easy-review/issues/606) Settings, session chrome and panels | D/U/P | opus | 🔵 | [#633](https://github.com/VilfredSikker/easy-review/pull/633), [#634](https://github.com/VilfredSikker/easy-review/pull/634), [#637](https://github.com/VilfredSikker/easy-review/pull/637), [#638](https://github.com/VilfredSikker/easy-review/pull/638) | P53.1 merged; P53.2 has merge verdicts and is merging release in (#635 conflicts in the routing files) |
| P54 | [#607](https://github.com/VilfredSikker/easy-review/issues/607) Tabs and the tab lifecycle | S/D/U/P | opus | 🔵 | [#642](https://github.com/VilfredSikker/easy-review/pull/642), [#644](https://github.com/VilfredSikker/easy-review/pull/644), [#645](https://github.com/VilfredSikker/easy-review/pull/645), [#646](https://github.com/VilfredSikker/easy-review/pull/646) | four stacked PRs on #638, in review; A15 read as same PR number plus owner/repo (#607) |
| P55.1 | [#608](https://github.com/VilfredSikker/easy-review/issues/608) The sidebar owner, projects and branches(project) | DS/US/P/D | opus | ✅ | [#632](https://github.com/VilfredSikker/easy-review/pull/632), [#635](https://github.com/VilfredSikker/easy-review/pull/635) | merged |
| P55.2 | [#609](https://github.com/VilfredSikker/easy-review/issues/609) prs(remote) keyed on content, head_oid included | DS/US/U/P/D | opus | 🔵 | [#641](https://github.com/VilfredSikker/easy-review/pull/641), [#643](https://github.com/VilfredSikker/easy-review/pull/643) | P55.2a (backend, two PRs) has merge verdicts and waits for P53.2; P55.2b (desktop-ui) follows P53.2 |
| P55.3 | [#610](https://github.com/VilfredSikker/easy-review/issues/610) inbox and the inbox optimistic ops | DS/US/U/P/D | opus | ⬜ | | |
| P56.1 | [#611](https://github.com/VilfredSikker/easy-review/issues/611) diff(tab), loading on activation, the warmer and the watcher | S/D/U/P/C | opus | ⬜ | | |
| P56.2 | [#612](https://github.com/VilfredSikker/easy-review/issues/612) Content-addressed hunks, previews and file reads | S/D/U/P | opus | ⬜ | | |
| P56.3 | [#613](https://github.com/VilfredSikker/easy-review/issues/613) History, Conflicts and Hidden in the service | S/D/U/P | opus | ⬜ | | |
| P57 | [#614](https://github.com/VilfredSikker/easy-review/issues/614) reviewed(tab) and frontend navigation | S/D/U/P | opus | ⬜ | | |
| P58.1 | [#615](https://github.com/VilfredSikker/easy-review/issues/615) annotations(tab) and ui_annotations(tab) | S/D/U/P | opus | ⬜ | | |
| P58.2 | [#616](https://github.com/VilfredSikker/easy-review/issues/616) Annotation writes and pending until ack | S/D/U/P | opus | ⬜ | | |
| P59 | [#617](https://github.com/VilfredSikker/easy-review/issues/617) pr(tab), pr_status, the stack and the GitHub senders | S/D/DS/U/P | opus | ⬜ | | |
| P60.1 | [#618](https://github.com/VilfredSikker/easy-review/issues/618) Arena and review flows on jobs | S/D/U/P | opus | ⬜ | | |
| P60.2 | [#619](https://github.com/VilfredSikker/easy-review/issues/619) AI runs, triage and task logs on jobs | S/D/DS/U/P | opus | ⬜ | | |
| P61 | [#620](https://github.com/VilfredSikker/easy-review/issues/620) Window-mechanics commands typed and moved out of commands.rs | D/S/U/P/C | opus | ⬜ | | |
| P62.1 | [#621](https://github.com/VilfredSikker/easy-review/issues/621) Delete the old store layer | U/P/C/D | opus | ⬜ | | |
| P62.2 | [#622](https://github.com/VilfredSikker/easy-review/issues/622) Delete the old desktop path and the desktop flag | D/DS/C/P/U | opus | ⬜ | | |

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
| Phase 4: core and service | [#397](https://github.com/VilfredSikker/easy-review/issues/397) | ✅ passed 2026-11-14; [ADDENDUM-05](./ADDENDUM-05-phase-4.md), issues #526–#538 |
| Phase 5: desktop protocol | [#398](https://github.com/VilfredSikker/easy-review/issues/398) | 🟡 gate passed 2026-11-18; building (ADDENDUM-06, #602–#622) |
| Phase 5b: TUI | [#399](https://github.com/VilfredSikker/easy-review/issues/399) | 🟡 draft plan [#640](https://github.com/VilfredSikker/easy-review/pull/640) in review |
| Phase 6: consolidate | [#400](https://github.com/VilfredSikker/easy-review/issues/400) | ⬜ |

## Waiting on the owner

**Decisions.** D1–D10 are settled in [ADDENDUM-01](./ADDENDUM-01-decisions.md) and D11–D18 in [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md). Routine merges and design approvals are delegated to the coordinator (BUILD_LOG, 2026-11-06). None is open.

**Actions.**

None open. The real 409 and 422 `gh` captures are taken (in `~/er-gh-fixtures/`, recorded on #381); the 403 rate limit stays synthetic.
