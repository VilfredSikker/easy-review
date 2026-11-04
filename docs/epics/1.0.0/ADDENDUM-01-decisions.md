# Addendum 01: owner decisions D1–D10

Recorded 2026-11-04. Where this file and [PLAN.md](./PLAN.md) disagree, this file wins.

The owner chose D1–D3. For D4–D10 the owner asked for the option that serves the 1.0.0 release best on stability and performance; the orchestrator chose, and the reasons are below.

## Chosen by the owner

- **D1 Browser annotations: defer to the phase 2 gate.** Phase 0 deletes nothing in `ai/browser_annotations*` or `ai/browser_prompt.rs`. At the phase 2 gate: if `release/v1.0.0` still has no caller, delete them; if it has one, `browser-annotations.json` becomes an `Artifact` in phase 3.
- **D2 Release line: keep v0.6.x on `release/v0.6.5`.** `release/v0.6.5` is cut from `main`. The resolver in `CLAUDE.md` on `main` skips `release/v1.0.0`, so ordinary feature work keeps targeting the v0.6.x line. Epic sessions take their branch from STATUS. Merge `main` into `release/v1.0.0` at every phase gate and after every v0.6.x release.
- **D3 TUI review submission: accept the gap until phase 5b.** After P05 and P06 the TUI posts single comments but cannot submit a review with a decision. The 1.0.0 release notes must say so if 5b has not restored it.

## Chosen by the orchestrator

- **D4 Keep the lenient Finding and Confidence readers.** They are how a load survives malformed model output, and `backfill_finding_lenses` gives general-review findings their lens. Deleting them makes loads fail on output agents still produce. Drop the item from the §5 list.
- **D5 Agent-written artifacts are bare payloads until phase 3.** The store validates them against the type and `diff_hash` but does not require an envelope. Phase 3's exit criteria delete this mode.
- **D6 Config moves with the root into `v1/`.** §3.7 makes config loading fail loudly. A v0.6 config still has `agent.effort`, `[importance.<repo>]` and legacy keys that phase 0 deletes, so reading it in place would show every upgrading user an error on first launch. A fresh file avoids that. The old file stays where it is, and the first-launch notice (P15) names it.
- **D7 Spawn guardrails land at error tier with a file allow-list, not a budget.** Seeding the budget would make `clippy-warning-budget.json`'s first real entries, which `CLAUDE.md` and ADR 0038 rule out. Instead P07 adds `er::git_spawn` at error tier, with an allow-list of the files that spawn `git` today under `[workspace.metadata.lint-gate]`. The list may only shrink, and a new spawn anywhere else fails at once. P08 and P09 empty it, and P09 deletes the list. P10 to P12 do the same for `er::gh_spawn`, and P16 already uses this pattern for `er::sidecar_fs`. The reviewer of each PR checks that the list only lost entries.
- **D8 No action.** `origin/chore/remove-dead-agent-runtime` belongs to PR #227, which merged into `release/v0.4.19` on 2026-09-14. `agent_runtime.rs` was restored and changed afterwards, in v0.5.x. The branch is stale, threatens nothing, and is not part of this epic.
- **D9 Codegen is decided at the phase 5 gate. The lean is `ts-rs`.** The new desktop surface has about three commands (`send`, `fetch`, `subscribe`), so typed command bindings buy little. Generating the types is what matters, and `ts-rs` adds fewer moving parts to the Tauri build.
- **D10 One copy of the proposal.** Merge PR #365 into `main`. At the first merge of `main` into `release/v1.0.0`, replace `PROPOSAL.md` with a link to `docs/plans/plan-clean-slate-architecture.md`. Until then `PROPOSAL.md` is the copy agents read. Merging #365 waits on the owner's go-ahead.

## Changes to PR entries

- **P07, P09, P10, P12:** replace "budgeted and seeded" with the D7 allow-list. P07 and P10 do not run `just clippy-accept`.
- **P15:** the notice also names the old `config.toml`.
- **Phase 3 exit criteria:** add "the D5 bare-payload mode is deleted". This was already implied; it is now explicit.
