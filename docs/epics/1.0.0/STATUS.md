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
| P07 | Create er-git | C | opus | ⬜ | | after P03; D7 |
| P08 | Move engine git calls behind the port | E | sonnet | ⬜ | | after P06, P07 |
| P09 | Move front-end git calls and close the git guardrail | F, C | sonnet | ⬜ | | after P08 |
| P10 | Create er-github | C | opus | ⬜ | | after P07; D7 |
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

**Decisions.** The default in the last column is what the work uses until the owner answers. Details in PLAN.md "Owner decisions".

| #   | Question | Default used for now |
| --- | -------- | -------------------- |
| D1 | Browser annotations: the v2 callers (PRs 352, 353) are stranded on `release/v0.6.1`, which never reached `main`. Delete the modules, or merge v0.6.1 and resume that epic? | No phase 0 deletion. Decide at the phase 2 gate. |
| D2 | Parallel release line: the resolver in `CLAUDE.md` now picks `release/v1.0.0` for all feature work. | Keep v0.6.x on `release/v0.6.5`; narrow the resolver to `release/v0.*` on `main`; merge `main` into `release/v1.0.0` at each phase gate and after each v0.6.x release. |
| D3 | After P05/P06 the TUI can post single comments but cannot submit a review with a decision. | Accept until phase 5b; note it in the release notes. |
| D4 | The Finding/Confidence "back-compat" deserialisers are live code that tolerates bad model output. | Drop the item from §5; delete nothing. |
| D5 | Agent-written sidecars cannot carry the envelope until phase 3. | The store reads them as bare payloads, validated, until the runner commits outputs. |
| D6 | Config sits under the storage root, so the `v1/` root moves config too. | Move it; settings are re-entered once. |
| D7 | Seed the lint budget for the new git/gh spawn rules, or land them only at zero? | Seed; each later PR may only lower the count. |
| D8 | `origin/chore/remove-dead-agent-runtime` deletes `agent_runtime`, which the design keeps. | Close it unmerged. |
| D9 | `tauri-specta` or `ts-rs`. | Decide at the phase 5 gate. |
| D10 | PR #365 holds the same proposal text against `main`. | Merge it, then replace PROPOSAL.md here with a link. |

**Actions.**

1. Answer D1–D10, or accept the defaults.
2. Before P10 merges: provide real `gh` error samples (404, 401, 403 rate limit, 409, 422) if the implementer cannot produce them from a test repo.
