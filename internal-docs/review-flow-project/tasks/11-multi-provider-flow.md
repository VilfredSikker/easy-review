# 11 — Spread the flow across providers

Status: ready-for-agent
Idea: 13 in the original idea list (summarised in ../README.md)
Depends on: 07-default-review-flow.md (the review flow must exist)
Impact: M · Complexity: M · Size: S
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.
>
> Task 07 names things this task builds on (`ReviewFlow`, `plan_for_triage`, `App::advance_review_flows`, `App::arena_start_seeded_for`, `App::arena_start_for`). Before Step 1, confirm they exist: `grep -rn "fn plan_for_triage\|fn advance_review_flows\|fn arena_start_seeded_for\|fn arena_start_for" crates/er-engine/src`. If any is missing or named differently, read task 07's `## Comments` for what was built, and stop and ask if it still does not match.

## In plain terms

- **The problem.** The review flow from task 07 runs every finder and the arbiter on the one provider the user picked as default. When the same model finds a bug and then grades it, it tends to agree with itself, and every finder shares the same blind spots.
- **Why it matters.** Different providers miss different bugs. Mixing them covers more ground for the same number of runs, and an arbiter from another provider is a real second opinion.
- **The fix.** When more than one provider CLI is installed, the flow gives each lens a different provider at Standard, two or more providers per lens at Deep, and picks the arbiter from a provider that wrote few of the findings. Each finding says how many distinct providers raised it. With only one provider installed, the flow still runs and says it had no cross-provider check.
- **TL;DR.** Use every installed provider in the flow, and show when a finding was confirmed by more than one.

## Source

From the blog, https://claude.dev/blog/solving-code-review-at-anthropic/:

> Several agents look for bugs at the same time. Then a separate group votes on each finding, and it only gets posted if enough of them agree it's real.

From the Code Review docs, https://code.claude.com/docs/en/code-review:

> 2. **Verify**: a verification step checks the candidate findings against the code and filters out false positives.

`docs/ai-provider-compliance.md` records why taking reviews from several providers and reconciling them is allowed, and the line not to cross (no training on aggregated outputs).

## Mission & definition of done

The flow picks providers from the ones whose CLI is on `PATH` (narrowed by an optional allowlist in global config), assigns them across lenses, chooses the arbiter from a provider other than the one that ran most finders, records which provider ran which lens, and reports for each verified finding how many distinct providers raised it.

- [ ] An engine function reports which configured providers have their command on `PATH`.
- [ ] Standard assigns lenses round-robin across installed providers, the default provider first.
- [ ] Deep puts each lens on two providers when two or more are installed.
- [ ] The arbiter comes from a provider other than the one that ran the most finders, when one exists.
- [ ] The seeded run's reviewer records name the provider and model that actually ran each lens.
- [ ] Each arbiter verdict carries the distinct providers that raised the finding; both front ends show the count.
- [ ] With one installed provider, the flow runs as in task 07 and its done line says "single provider".
- [ ] `ai_hub.flow_providers` in global config limits which providers the flow may use; empty means all installed.
- [ ] Quorum (ADR 0024) is unchanged.
- [ ] `just test`, `just test-ui`, `just clippy`, `just lint-ui`, `just docs-check`, `just comments-check` pass.

**Risk:** ordinary. The worst case is sending a diff to a provider the user did not expect; the allowlist and the done line guard against it.

## Before you start (orientation)

- **Branch:** the current release branch; see `../README.md` "Branching".
- **Commands** (one at a time, each through the lock):
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow`
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine seeded`
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine env_path`
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine flow_providers`
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine arbiter`
  - `~/.claude/scripts/locked.sh . just test`, `~/.claude/scripts/locked.sh . just test-ui`
  - `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`
- **The map** (confirmed paths):
  - `crates/er-engine/src/ai_hub_catalog.toml` — providers `claude` (:6), `codex` (:52), `cursor` (:131), `opencode` (:164); `default_provider = "claude"`, `default_model = "sonnet-5.5"` (:3-4).
  - `crates/er-engine/src/config/mod.rs` — `AiHubConfig` (:348-386), `AiProviderConfig` (:487, `command` at :491), `resolve_provider_id` (:672), `resolve_model_id` (:687), `supplement_ai_hub` (:918), which seeds every catalog provider whether or not it is installed.
  - `crates/er-engine/src/env_path.rs` — `init_cli_path` (:11) widens `PATH` for GUI launches; the new `PATH` lookup goes here.
  - `crates/er-engine/src/model_discovery.rs` — runs a provider's `models_command`; it lists models and does not check sign-in.
  - `crates/er-engine/src/arena/orchestrator.rs` — `default_arbiter_from_hub` (:79-96), `resolve_arbiter` (:106), `estimate_cost_usd` (:134), `SeededStartParams.arbiter` (:432), `arbiter_review_from_run` (:571-609), `resolve_reviewers` (:1487). Tests at :1556, including `default_arbiter_picks_most_expensive_model` (:1689).
  - `crates/er-engine/src/arena/seeded.rs` — `build_seeded_run` (:113), which labels every reviewer with the hub default (:118-131).
  - `crates/er-engine/src/arena/model.rs` — `ReviewerRef` (:76), `Reviewer` (:102, has `provider_id`), `ArenaFinding.raised_by` (:152).
  - `crates/er-engine/src/ai/arbiter.rs` — `ArbiterVerdict` (:41-60), `apply_verdict` copying `raised_by` onto the finding (:185-200).
  - `crates/er-engine/src/ai/review.rs` — `Finding` (:271), `raised_by` (:289).
  - `crates/er-engine/src/app/state/mod.rs:5934` — `set_ai_selection_override`; `crates/er-engine/src/app/state/comments.rs:3084-3093` — the queue snapshots the selection at enqueue.
- **Conventions to follow.** Per-spawn provider choice through the queue, from the test `excess_reviews_queue_and_dedup` (`crates/er-engine/src/app/state/comments.rs:4074-4081`):

  ```rust
  app.set_ai_selection_override(AiSelection {
      provider_id: Some("codex".into()),
      model_id: Some("gpt-5.3-codex-spark".into()),
      effort: None,
  });
  app.spawn_background_triage_review(target(&tmp, "feat-b"), "p".into(), true)
      .unwrap();
  app.clear_ai_selection_override();
  ```

  The current arbiter pick (`crates/er-engine/src/arena/orchestrator.rs:79-96`), which considers every configured provider:

  ```rust
  pub fn default_arbiter_from_hub(hub: &crate::config::AiHubConfig) -> Option<ReviewerRef> {
      let mut best: Option<(f32, ReviewerRef)> = None;
      for (provider_id, provider) in &hub.providers {
          for model in &provider.models {
              let cost =
                  model.cost_per_1k_in.unwrap_or(0.015) + model.cost_per_1k_out.unwrap_or(0.075);
  ```
- **Mental model.** er has no provider client (ADR 0031): each provider is a CLI command in `ai_hub.providers`, and the user is signed in to it already. The catalog merges every shipped provider into the hub on load (ADR 0042), so the hub lists Codex, Cursor and OpenCode even on a machine that has only `claude`. Background spawns take their provider from an `AiSelection` captured at enqueue; arena reviewers and the arbiter are exact `ReviewerRef`s. A seeded run spawns no finders; its `reviewers` list only labels who produced the expert sidecars.
- **ADRs to read first:**
  - 0031 — er drives the user's CLI; adding a provider is a catalog entry.
  - 0042 — the catalog owns preset models; never write presets, or anything derived from them, back to config.
  - 0005 — config is global-only; the allowlist goes in global config.
  - 0024 — quorum and explicit accept. This task does not change quorum.
  - 0035 — the arbiter overlay; `arbiter.json` gains one optional field here.
  - 0021 — the slot cap is unchanged; several providers still share the same pool.

## Scope boundaries

- **In scope:** the `PATH` check, the allowlist, lens-to-provider assignment at Standard and Deep, the arbiter pick, honest reviewer records on the seeded run, the distinct-provider count on verdicts and its display.
- **OUT of scope / DO NOT TOUCH:**
  - Quorum semantics (ADR 0024). Counting is display only.
  - Checking whether a CLI is signed in. A spawn that fails for that reason is a failed finder, which task 07 already handles.
  - A per-project allowlist (ADR 0005 forbids repo-local config).
  - Choosing models by speed or price beyond what `resolve_model_id` and `estimate_cost_usd` already do.
  - Using provider outcomes to route or score providers (later, after task 03; never training — see `docs/ai-provider-compliance.md`).
  - Running two providers on one lens through the expert path at Standard. Expert sidecars are one file per lens (`experts/<id>.json`); a second provider on the same lens overwrites the first.

## Steps

### Step 1 — Know which providers are installed

- **Goal:** `pub fn installed_providers(hub: &AiHubConfig) -> Vec<String>`, provider ids whose `command` resolves on `PATH`, default provider first, then the rest in `BTreeMap` order.
- **Why / invariant:** `supplement_ai_hub` puts every catalog provider in the hub (`crates/er-engine/src/config/mod.rs:918-928`), so presence in config says nothing about installation. Nothing checks it today (`grep -rn "fn .*installed" crates/er-engine/src` finds only `ensure_gh_installed`, `crates/er-engine/src/github.rs:281`, which spawns `gh`). The check must spawn nothing, so it is a file lookup.
- **Files:** `crates/er-engine/src/env_path.rs` (add `pub fn command_on_path(command: &str) -> bool`); a new `crates/er-engine/src/ai/flow_providers.rs` for `installed_providers`, registered in `crates/er-engine/src/ai/mod.rs` like `triage` (`pub mod` at :14, `pub use` at :30). `ai/` is always compiled; `arena` and `app` are `ui`-gated (`crates/er-engine/src/lib.rs:9-12`), so this file must not use them.
- **Do this:**
  - `command_on_path`: if `command` contains a `/`, check that path is a file; otherwise walk `std::env::split_paths(&std::env::var_os("PATH"))` and return true on the first `dir.join(command)` that is a file (on unix, also executable: `std::os::unix::fs::PermissionsExt`, mode `& 0o111 != 0`).
  - `installed_providers`: filter `hub.providers` by `command_on_path(&p.command)`, then by `hub.flow_providers` when non-empty (Step 2), then order default first.
  - Tests first: a temp dir with an executable file put on `PATH` for the test, a missing command, an absolute path. `PATH` is process-wide; guard the tests with a static mutex, as `crates/er-engine/src/arena/adapter.rs:323` does for `ER_FAKE_ARENA_DIR`.
- **Watch out for:** the desktop is a GUI app; `init_cli_path` (`env_path.rs:11`) must have run before the lookup, which it does at startup. Do not call it again from here.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine env_path` → new tests pass.
- **If it fails:** a test that passes alone and fails in the suite is the shared `PATH`; take the mutex.

### Step 2 — Global allowlist

- **Goal:** `ai_hub.flow_providers: Vec<String>` in global config; empty means every installed provider.
- **Why / invariant:** the flow will send a diff to every installed provider, and a user may have a CLI installed for other work that they do not want reviewing this code. Config is global-only (ADR 0005). Nothing about installed state is persisted (ADR 0042).
- **Files:** `crates/er-engine/src/config/mod.rs` — `AiHubConfig` (:348); add the field with `#[serde(default)]` and a doc comment saying why.
- **Do this:** add the field; unknown ids in it are ignored. No Settings UI in this task; the TOML key is enough. `save_config` serializes a clone of the whole config through `persisted_config` (`crates/er-engine/src/config/mod.rs:1855-1867`), which only strips preset models, so a new `#[serde(default)]` field survives a save. Settings lists its fields by hand (`crates/er-engine/src/config/settings.rs:193`, `crates/er-engine/src/config/desktop_settings.rs:348`); leaving the key out of those lists only means Settings does not show it.
- **Verify:** a config test that a TOML with `flow_providers = ["claude"]` round-trips through load and save. `~/.claude/scripts/locked.sh . cargo test -p er-engine config` → passes.
- **If it fails:** a save that drops the key means the save path rebuilds `AiHubConfig` field by field. Follow that path; do not special-case the key.

### Step 3 — Assign providers to lenses

- **Goal:** a pure function that turns a plan and the installed providers into one or more `ReviewerRef`s per lens.
- **Why / invariant:** Standard gets diversity for no extra runs; Deep gets two providers on each lens. With one provider the result equals task 07's.
- **Files:** `crates/er-engine/src/ai/flow_providers.rs` (from Step 1). Return plain `(provider_id, model_id)` pairs: `ReviewerRef` is an arena type behind the `ui` feature (`crates/er-engine/src/arena/model.rs:76`, `crates/er-engine/src/lib.rs:11-12`), and the driver in `app/state/review_flow.rs` turns the pairs into `ReviewerRef`s or `AiSelection`s.
- **Do this:**
  - `pub fn assign_lenses(depth: FlowDepth, lenses: &[String], providers: &[String], hub: &AiHubConfig) -> Vec<(String, Vec<(String, String)>)>` returning, per lens, the `(provider_id, model_id)` pairs.
  - Standard and Quick: lens `i` gets `providers[i % n]`.
  - Deep: lens `i` gets `providers[i % n]` and `providers[(i + 1) % n]` when `n >= 2`, otherwise just the one.
  - Model: `hub.resolve_model_id(provider_id, None)` (`config/mod.rs:687`), the same fallback every ordinary spawn uses. For the default provider this is `default_model`; for others it is the provider's first model.
  - Tests: one provider (all lenses on it), two providers and three lenses at Standard, two providers at Deep (each lens gets both, leading provider rotates), empty providers (returns the hub default for every lens).
- **Watch out for:** for a non-default provider, the first model in catalog order is often its most expensive (Codex's first is `gpt-6-astra`, `ai_hub_catalog.toml:61-67`). That is what the existing resolver does; do not add a new picking rule. The Deep estimate prices whatever is chosen.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine flow_providers` → all pass; `~/.claude/scripts/locked.sh . just build-engine-headless` → exits 0.
- **If it fails:** a headless build error means a `ui`-gated type leaked in.

### Step 4 — Wire the assignment into the flow and pick the arbiter

- **Goal:** the flow spawns each lens on its assigned provider and picks a cross-provider arbiter.
- **Why / invariant:** a model should not grade its own work when another provider is available. Every spawn still goes through the queue or the arena (ADR 0021).
- **Files:** `crates/er-engine/src/app/state/review_flow.rs` (task 07's driver); `crates/er-engine/src/arena/orchestrator.rs:79` (beside `default_arbiter_from_hub`).
- **Do this:**
  - Standard and Quick: before each `spawn_background_review` / `spawn_background_expert_review`, call `self.set_ai_selection_override(AiSelection { provider_id, model_id, effort: None })`, and `self.clear_ai_selection_override()` right after, even on error.
  - Deep: build the arena `reviewers` from every pair: `ReviewerRef { provider_id, model_id, agent_kind: Some(kind) }`. `resolve_reviewers` makes ids `"{agent_kind}::{provider}-{model}"` (`orchestrator.rs:1524-1528`), so two providers on one lens get distinct ids.
  - Add `pub fn cross_provider_arbiter(hub: &AiHubConfig, installed: &[String], finder_providers: &[String]) -> Option<ReviewerRef>`: count finders per provider, take the most common, and pick the most expensive model (same cost formula as :83-84) among installed providers other than that one; if none, the most expensive model among installed providers; if no provider is installed, `default_arbiter_from_hub(hub)`. Pass the result as `SeededStartParams.arbiter` (:432) and `ArenaStartParams.arbiter` (:51).
  - Record `providers_used: Vec<String>` on `ReviewFlow` and set it from the assignment.
  - Tests for `cross_provider_arbiter` next to `default_arbiter_picks_most_expensive_model` (:1689): two providers, finders mostly on `a` → arbiter from `b`; one provider → its most expensive model; an uninstalled expensive provider is never picked.
- **Watch out for:**
  - `default_arbiter_from_hub` stays as it is; the arena launcher uses it. Today it ties Fable 5.1 (Claude) and gpt-6-astra (Codex) on cost, and the strict `>` keeps the first in `BTreeMap` order (`claude`). The new function must not rely on that tie.
  - The override is snapshotted at enqueue (`comments.rs:3084-3093`), so setting it before each spawn is enough even when the task queues.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine orchestrator` and `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass. A driver test with two fake providers whose commands are both `true` asserts each queued task's `ai_selection.provider_id` (pattern: `pending_background_tasks[0].ai_selection`, `comments.rs:4085-4091`).
- **If it fails:** a task that ran on the default provider means the override was cleared before the enqueue.

### Step 5 — Record who ran each lens on the seeded run

- **Goal:** the seeded run's `reviewers` name the provider and model that produced each lens's sidecar.
- **Why / invariant:** today `build_seeded_run` labels every reviewer with the hub default (`crates/er-engine/src/arena/seeded.rs:118-131`, whose comment calls a wrong label "a lie the process matrix repeats"). With several providers that label becomes wrong, and Step 6 counts providers from it.
- **Files:** `crates/er-engine/src/arena/seeded.rs:99-107` (`SeededRunParams`), :113 (`build_seeded_run`); `crates/er-engine/src/arena/orchestrator.rs:418-433` (`SeededStartParams`).
- **Do this:** add `lens_providers: BTreeMap<String, ReviewerRef>` to both params structs (empty = today's behaviour). In `build_seeded_run`, use the entry for each contributing lens when present, else the hub default as now. The flow passes its assignment. `App::arena_start_seeded` (`crates/er-engine/src/app/state/arena.rs:173`) passes an empty map.
- **Watch out for:** the test `a_seeded_run_lists_its_contributing_experts_as_reviewers` (`seeded.rs:644`) must pass unchanged with the empty map. Task 07 Step 5.4 already added `include_general: bool` to `SeededStartParams` and set it in its two struct literals (`crates/er-engine/src/app/state/arena.rs:200`, `crates/er-engine/tests/seeded_arbiter.rs:124`); add `lens_providers` beside it in the same literals. 07 Step 4 maps the seeded lens `general` to `agent_kind: Some("general")`, so key the map by lens id (`general`, `security`, …) to match.
- **Verify:** a new test beside it: with `lens_providers` mapping `security → codex` and `testing → claude`, the reviewers carry those provider ids. `~/.claude/scripts/locked.sh . cargo test -p er-engine seeded` → all pass.
- **If it fails:** `resolve_reviewers` errors "unknown provider" when a test hub lacks the provider; call `crate::config::supplement_ai_hub(&mut config.ai_hub)` as the existing test does (:662).

### Step 6 — Count distinct providers per finding and show it

- **Goal:** each arbiter verdict lists the distinct providers that raised the finding; the review shows "N providers" on the row.
- **Why / invariant:** two Claude agents agreeing is weaker evidence than Claude and Codex agreeing. This is display only; the gate and quorum do not read it.
- **Files:**
  - `crates/er-engine/src/arena/` — `pub fn providers_for(finding: &ArenaFinding, reviewers: &[Reviewer]) -> Vec<String>`: map each entry in `raised_by` to a reviewer (by `id`, or by `agent_kind == "expert:<lens>"` / `"general"` for seeded lens ids) and collect distinct `provider_id`s, sorted.
  - `crates/er-engine/src/ai/arbiter.rs:41` — add `#[serde(default)] pub providers: Vec<String>` to `ArbiterVerdict`; fill it in `arbiter_review_from_run` (`orchestrator.rs:587-598`).
  - `crates/er-engine/src/ai/review.rs:271` — add `#[serde(default)] pub providers: Vec<String>` to `Finding` with a doc comment saying it is set at load by the arbiter overlay and never written by a producer; set it in `apply_verdict` (`arbiter.rs:185-200`) beside `raised_by`.
  - Desktop: the finding snapshot beside `lens_category` (`crates/er-desktop/src/snapshot.rs:4287`), the TS type in `desktop-ui/src/lib/types.ts`, and a short label in `desktop-ui/src/lib/components/InlineFinding.svelte` (`lensCategory` at :35) reading "2 providers" when `providers.length >= 2`.
  - TUI: `crates/er-tui/src/ui/panel.rs:598` (`push_finding` builds `lens_category_tag()`): append " · 2 providers" when two or more.
  - Flow done line (task 07's `advance_review_flows` notification and desktop state): when `providers_used.len() < 2`, say "single provider: no cross-provider check".
- **Watch out for:** `Finding` is AI-owned (ADR 0009). The new field is set only at load, like `lens`; nothing writes it back to `review.json`. Old `arbiter.json` files without `providers` must load (the `serde(default)`). `Finding` has no `Default`, so every struct literal needs `providers: Vec::new()`: `grep -rn "[^a-zA-Z]Finding {$" crates` lists 29 today (tasks 06 and 08 add `pre_existing`, `pattern` and `sites` to the same literals; keep theirs), and `grep -rn "ArbiterVerdict {" crates` lists 4 (including the test helper `verdict` at `crates/er-engine/src/ai/arbiter.rs:321`).
- **Verify:** tests: `providers_for` with arena ids and with seeded lens ids; an `arbiter.json` without `providers` still applies (next to `a_verdict_without_raisers_does_not_clear_the_findings_own`, `crates/er-engine/src/ai/arbiter.rs:473`). `~/.claude/scripts/locked.sh . cargo test -p er-engine arbiter` → all pass. `~/.claude/scripts/locked.sh . just test`, `~/.claude/scripts/locked.sh . just test-ui`, `~/.claude/scripts/locked.sh . just check-ui` → exit 0.
- **If it fails:** an empty `providers` list on seeded findings means Step 5's map was not passed; check the run's `reviewers` first.

## Gotchas & landmines

- Being in `ai_hub.providers` does not mean installed (`supplement_ai_hub`). Every provider choice in this task goes through `installed_providers`.
- "Signed in" cannot be checked without a spawn. A provider that is installed but signed out fails its finder. At Deep, two such failures can drop the arena below `MIN_QUORUM` and abort the run (`orchestrator.rs:1164-1187`); the flow reports that as `Failed` with the arena's reason.
- Distinct providers are counted by `provider_id`. OpenCode and Cursor can serve Anthropic or OpenAI models (`ai_hub_catalog.toml:184-220` lists `claude-sonnet-4-5`, `gpt-5.2` under OpenCode), so "two providers" can be one model vendor. Accepted for this task; see the decision log.
- `raised_by` holds lens ids on the seeded path and reviewer ids (`"{agent_kind}::{provider}-{model}"`) on the arena path. `providers_for` must handle both.
- `estimate_cost_usd` averages the reviewers' rates (`orchestrator.rs:145-152`), so a mixed set is priced as a mix. No change needed.

## Decision log (forks pre-answered)

- **Installed means on `PATH`.** No spawn, no sign-in check.
- **Diversity key is `provider_id`.** A model-vendor key would need a new catalog field and a rule for user models. If the owner wants counting by model vendor, stop and ask.
- **Model per provider:** `resolve_model_id(provider, None)`, the existing fallback. No new picking rule.
- **Allowlist:** global, per user, `ai_hub.flow_providers`. Empty = all installed. A per-project list would need repo-local config, which ADR 0005 rules out.
- **Quorum:** unchanged. The provider count is shown, never used to keep or drop a finding.
- **Arbiter:** from a provider other than the most common finder provider, most expensive model there. Ties in finder counts break toward the default provider being the "most common".

## Full verification

1. `~/.claude/scripts/locked.sh . cargo test -p er-engine env_path` → all pass.
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
3. `~/.claude/scripts/locked.sh . cargo test -p er-engine seeded` → all pass, existing tests unchanged.
4. `~/.claude/scripts/locked.sh . cargo test -p er-engine arbiter` → all pass.
5. `~/.claude/scripts/locked.sh . just build-engine-headless` → exits 0.
6. `~/.claude/scripts/locked.sh . just test` and `~/.claude/scripts/locked.sh . just test-desktop` → all pass.
7. `~/.claude/scripts/locked.sh . just test-ui` and `~/.claude/scripts/locked.sh . just check-ui` → exit 0.
8. `~/.claude/scripts/locked.sh . just clippy` and `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
9. Manual, on a machine with `claude` and `codex` installed: Review on a diff triage routes to `expert`. Expect the background list to show lenses on both providers, the arbiter on the provider that ran fewer finders, and "2 providers" on findings both raised. Then set `flow_providers = ["claude"]` in global config and run again: everything on Claude and the done line says "single provider".

## Rollback

One commit per step; revert in reverse. The new config key and the new `arbiter.json` / `Finding` fields are optional with `serde(default)`, so files written with them load after a rollback.

## When to STOP and ask

- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- Task 07's flow is missing or shaped differently than this file assumes.
- The owner wants agreement across providers to change what is kept (quorum, gate, or arbiter weighting). That amends ADR 0024 and needs an ADR first.
- The owner wants a per-project allowlist (conflicts with ADR 0005) or a sign-in check (needs a spawn per provider).
- The owner wants providers counted by model vendor instead of `provider_id`.
- er-mcp should use several providers. er-mcp spawns no agents; that is task 07's open question.

## Comments
