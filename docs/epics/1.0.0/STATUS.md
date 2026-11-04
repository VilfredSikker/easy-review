# Epic 1.0.0 status

> Release branch `release/v1.0.0`. Every PR below targets it.
> Plan: [PLAN.md](./PLAN.md). Design: [PROPOSAL.md](./PROPOSAL.md). Pick-up notes: [BUILD_LOG.md](./BUILD_LOG.md).
> Updated by the orchestrator only. Last update: 2026-11-03.

**Legend:** ⬜ planned · 🟡 in progress · 🔵 in review · 🔧 fixing review findings · ✅ merged · ⛔ blocked

## Progress

Keep the column order; `scripts/status.sh` edits columns by position. Lanes: **C** crates, config and lint gate; **E** engine state; **F** front ends.

| PR  | Title | Lane | Model  | Status | PR link | Notes |
| --- | ----- | ---- | ------ | ------ | ------- | ----- |
| P01 | Delete legacy feedback | E, F | opus | ⬜ | | |
| P02 | Delete legacy config paths and the effort fallback | C | opus | ⬜ | | |
| P03 | Delete the importance table and old tier words | C | opus | ⬜ | | after P02 |
| P04 | Delete the repo-local storage root | E, F | opus | ⬜ | | after P01 |
| P05 | Post GitHub comments when they are created | E, F | opus | ⬜ | | after P04; D3 |
| P06 | Delete the local review draft | E, F | opus | ⬜ | | after P05 |
| P07 | Create er-git | C | opus | ⬜ | | after P03; D7 allow-list |
| P08 | Move engine git calls behind the port | E | sonnet | ⬜ | | after P06, P07 |
| P09 | Move front-end git calls and close the git guardrail | F, C | sonnet | ⬜ | | after P08 |
| P10 | Create er-github | C | opus | ⬜ | | after P07; D7 allow-list |
| P11 | Move github.rs into er-github | E | opus | ⬜ | | after P08, P10 |
| P12 | Move front-end gh calls and close the gh guardrail | F, C | opus | ⬜ | | after P09, P11 |
| P13 | One remote-slug parser | - | sonnet | ⬜ | | after P11 |
| P14 | Create er-store | C | opus | ⬜ | | after P10; D5 |
| P15 | Move to the v1 root | E, C | opus | ⬜ | | after P04, P14; D6 |
| P16 | Comments and notes through the store | E | opus | ⬜ | | after P06, P11, P15 |
| P17 | Session state through the store | E | opus | ⬜ | | after P16 |
| P18 | Review artifacts through the store | E | opus | ⬜ | | after P17 |
| P19 | Remaining AI artifacts through the store | E | opus | ⬜ | | after P18 |
| P20 | Findings projection for experts and the arbiter | E | opus | ⬜ | | after P18 |
| P21 | Scoped re-review as its own artifact | E | opus | ⬜ | | after P20 |
| P22 | App records through the store | F | opus | ⬜ | | after P13, P15 |
| P23 | Cross-process locks and change notifications | C | opus | ⬜ | | after P14 |
| P24 | MCP uploads through the store | - | opus | ⬜ | | after P19, P23 |

Phases 3–6 are milestones in PLAN.md and are planned as PRs at each phase gate.

## Waiting on the owner

**Decisions.** D1–D10 are settled in [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md). None is open.

**Actions.**

1. Go-ahead to merge PR #365 into `main` (D10).
2. Before P10 merges: provide real `gh` error samples (404, 401, 403 rate limit, 409, 422) if the implementer cannot produce them from a test repo.
