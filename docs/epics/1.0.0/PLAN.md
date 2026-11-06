# Epic 1.0.0 plan

Source: [the proposal](../../plans/plan-clean-slate-architecture.md). Its "Decisions taken" table is settled and
is not reopened here. Owner decisions D1–D10 are settled in [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md), which wins where it disagrees with this plan. Resume notes: [BUILD_LOG.md](./BUILD_LOG.md). Progress: [STATUS.md](./STATUS.md).

Target branch: `release/v1.0.0`, cut from `main` at `604c7945d`, verified on
2026-11-03. Phases 0, 1 and 2 are planned as PRs. Phases 3 to 6 are milestones
and are re-planned against the merged tree at each phase gate. Owner decisions
recorded later in an addendum in this folder override this plan.

## Verified starting point

Each claim a PR below rests on was checked against the tree at `604c7945d`. Line
counts were not re-checked. These claims hold:

- `feedback.json` is still read by `ai/loader.rs` (legacy fallback) and written
  for migration in `ai/comments.rs`. `CommentRef::Legacy` has match arms in
  `ai/review.rs`, `ai/comments.rs`, `er-desktop/src/snapshot.rs` and
  `er-tui/src/ui/diff_view.rs`. `ErReview.feedback` exists.
- `legacy_config_candidates` and `migrate_legacy_config` exist in
  `config/mod.rs`, and `load_global_config` falls back to the legacy paths for a
  session when migration fails. `uninstall.rs` still lists the legacy
  `~/.cache/er` directory.
- `[importance.<repo>]` is read as risk rules (`ImportanceConfig`,
  `RiskRuleTable::from_importance`), and `config/risk_rules.rs` still parses
  `foundational`, `isolated` and `normal`. The importance prompt already emits
  only the new tiers.
- `agent.effort` is a live setting: it is read in `config/mod.rs` and exposed by
  `config/settings.rs`, `config/desktop_settings.rs` and the desktop
  `settingsSearch.ts`. `supplement_ai_hub` calls `retire_preset_models`.
- `ER_REPO_LOCAL` and `ErRoot::RepoLocal` are live across `paths.rs`,
  `storage.rs`, `export.rs`, `app/state/{mod,comments,preload,arena}.rs`,
  `app/state/review_flow/results.rs` and `er-desktop/src/snapshot.rs`. ADR 0003,
  `docs/config-reference.md` and `crates/er-engine/src/watch/CLAUDE.md` describe it.
- `submit_github_review` exists in `er-desktop/src/commands.rs` and is called by
  `CommentsCard.svelte`. `ArtifactBaseline::validate` has no production caller.
- `git` is spawned directly from engine `app/state/*`, `storage.rs`,
  `sidecar_upload.rs` and `ai/review_md.rs`, and from desktop `snapshot.rs`,
  `commands.rs`, `tabs.rs`, `projects.rs`, `main.rs` and `commands/line_commits.rs`,
  besides `git/status.rs` and `git/blame.rs`.
- `github.rs` and `gh_pr_actions.rs` each keep their own caches, and
  `github.rs` classifies a 404 by `stderr.contains("404")`.
- Remote-slug parsing is duplicated in `github.rs`
  (`parse_owner_repo_from_remote`), `projects_pins.rs`, `er-desktop/src/projects.rs`
  and `er-mcp/src/projects.rs`. `projects.json` has writers in `projects_pins.rs`
  and the desktop, and lives under `~/.config/er/`, outside the storage root.
- `schemars` is already a dependency of `er-mcp` only. `sidecar_specs.rs` exists.

These claims are false, partly false, or changed:

1. **Browser annotations have callers on another branch.** On this tree
   `ai/browser_annotations*` and `ai/browser_prompt.rs` have no caller outside
   the `ai/mod.rs` re-export. But PR 352 (ER35, MCP reader) and PR 353 (ER32b,
   desktop snapshot) were merged into `release/v0.6.1`, and `release/v0.6.1` was
   never merged into `main`: 13 commits and about 1,200 lines are stranded
   there, and v0.6.2 to v0.6.4 shipped without them. `epic/er22-parking-proof`
   is an open draft. ER23 to ER36 are blocked on macOS proofs. Last activity was
   2026-10-31. See decision D1.
2. **The Finding and Confidence "back-compat deserialisers" are mostly live
   code.** There is no custom `Deserialize` impl. `lenient_findings`,
   `lenient_line_anchor`, `lenient_verdicts` and `lenient_risk_level` tolerate
   malformed model output, which agents still produce. `backfill_finding_lenses`
   runs on every load and is how a general-review finding gets its `lens`,
   because producers never write that field. Deleting any of them is a
   regression. See decision D4.
3. **Deleting the unpushed-local-comment path removes the TUI's only way to post
   to GitHub.** The TUI has no pending-review path; it posts through
   `push_comments_as_review` and `push_all_comments_to_github`
   (`er-tui/src/input/mod.rs`). On the desktop, `push_github_comment_thread` and
   `push_github_comment_reply` (from `InlineThread.svelte` and
   `ReplyActionBar.svelte`) push the same unsynced state. The `synced` flag
   appears in engine, desktop, TUI and desktop-ui. This is a two-PR stack, and
   decision D3 covers the TUI.
4. **There is a third gh layer in the desktop.** `er-desktop/src/gh_pr_actions.rs`,
   `pr_cache.rs`, `gh_status_cache.rs`, `github_image.rs`, `projects.rs`,
   `main.rs` and the `run_gh_pr_*_for_open` helpers in `commands.rs` spawn `gh`
   directly. `commands.rs` also matches `"422"` in stderr. The er-github scope
   includes all of them.
5. **`AgentTaskKind` is already partly wired.** `app/card_ai_spawn.rs` resolves
   argv through `agent_runtime::resolve_invocation`. Only
   `ArtifactBaseline::validate` lacks a production caller. The unmerged branch
   `origin/chore/remove-dead-agent-runtime` deletes `agent_runtime`, which the
   proposal keeps; it must not be merged (D8).
6. **`flows.json` holds diagram output** (`ai/diagrams.rs`, `diagram_upload.rs`),
   not workflow state. Review-flow state is not persisted; only the arena
   persists its run. Phase 3 needs a new workflow record.
7. **Config lives under the storage root.** `managed_config_path()` is
   `storage_root().join("config.toml")`. Moving the root to `v1/` moves config,
   which matches §5 ("settings are re-entered") but changes the path `CLAUDE.md`
   states (D6).
8. **Agent-written sidecars cannot carry an envelope until phase 3.** The agent
   writes `review.json`, `triage.json`, expert, tour and diagram files itself
   (ADR 0031), and external agents write into bucket directories too. Until the
   runner commits outputs (phase 3), the store must read those kinds as a bare
   payload (D5).
9. **§6 assigns the config split (§3.7) to no phase.** This plan puts
   `agents.toml` with phase 3 and the settings schema with phase 4.
10. **Minor drift.** There are five `now_epoch_ms` copies, three in the desktop
    and one each in `projects_pins.rs` and `commands.rs`. `ReachLevel::Isolated`
    in `ai/triage.rs` (and `triageCard.ts`) is a reach level; the tier-spelling
    deletion must not touch it.

## Lint gate facts the guardrails rely on

- There is one `clippy.toml`, at the root. Clippy reads the nearest
  `clippy.toml` walking up from the crate directory and does not merge files,
  so a per-crate file must repeat the five root keys. No PR in phases 0 to 2
  adds one.
- `disallowed_methods` cannot match arguments, so it cannot express
  `Command::new("git")`. `er-lint-gate` already appends syn-based diagnostics
  (`er::impl_cohesion`, from `cohesion.rs`) to clippy's. The new rules follow
  that pattern: a syn walk in a new module of `crates/er-lint-gate/src/`, a
  diagnostic code `er::<rule>`, and configuration under
  `[workspace.metadata.lint-gate]` in the root `Cargo.toml`.
- `clippy-warning-budget.json` is `{}` today: every budgeted lint is at zero.
  Seeding a new rule makes the first real entries (D7).
- `er-lint-gate` runs `cargo clippy --workspace --all-targets` with default
  features, so a non-default Cargo feature is never linted (see the build flag).
- There is no `deny.toml`. Dependency-direction rules are left to phase 6.

## Lanes

At most three lanes run at once, and every heavy check goes through
`~/.claude/scripts/locked.sh <worktree> just <recipe>`. A lane is a set of hot
files. A PR holds every lane whose files it touches, and starts only when all of
them are free.

| Lane | Hot files it owns |
|---|---|
| **E** engine state | `crates/er-engine/src/app/state/mod.rs`, `app/state/comments.rs`, `app/state/github_sync.rs`, `ai/review.rs`, `ai/comments.rs`, `ai/loader.rs`, `sync.rs`, `github.rs` |
| **F** front ends | `crates/er-desktop/src/commands.rs`, `snapshot.rs`, `main.rs`, `desktop-ui/src/lib/types.ts`, `crates/er-tui/src/input/mod.rs`, `crates/er-tui/src/ui/diff_view.rs` |
| **C** crates, config, gate | root `Cargo.toml`, `Cargo.lock`, `justfile`, `scripts/docs-check.sh`, `crates/er-lint-gate/**`, `crates/er-engine/src/config/mod.rs`, `config/risk_rules.rs` |

Rules:

- Crate-creating PRs (P07, P10, P14) run in sequence because they edit
  `Cargo.toml` and `Cargo.lock`.
- `clippy-warning-budget.json` belongs to no lane. After a rebase, regenerate it
  with `just clippy-prune` instead of resolving its conflicts.
- Phases 0, 1 and 2 overlap where the dependencies allow. The hard gate is
  before phase 3.

## Acceptance commands

Every command runs as `~/.claude/scripts/locked.sh <worktree> just <recipe>`,
one at a time. Each PR names its own set; all PRs run `fmt-check` and `clippy`.

- Rust in `er-engine` or `er-tui`: `test`. Also `build-engine-headless` when the
  engine's dependencies or features change, because `er-mcp` builds it headless.
- Rust in `er-desktop`: `test-desktop`.
- Rust in `er-mcp`, or a new crate before it joins `test`: `test-all`.
- `desktop-ui`: `check-ui`, `lint-ui`, `test-ui`.
- Docs or ADRs: `docs-check`. Inline comments changed: `comments-check`.

`just test` runs only `-p er-engine -p er-tui`. Each crate-creating PR adds its
crate to that recipe, or its tests never run locally. CI runs `cargo test` over
the whole workspace, and runs on PRs into `release/**`.

## Phase 0: delete

Old data needs no compatibility read path, so each item is a plain deletion.
The ADR or agent-doc edit a deletion forces lands in the same PR. Browser
annotations (D1) and the Finding/Confidence item (D4) are not scheduled here.

### P01 Delete legacy feedback

- Branch: `epic/p01-legacy-feedback`. Lanes: E, F. Model: `opus`.
- Depends on: none.
- Scope: remove the `feedback.json` read in `ai/loader.rs` and its writer in
  `ai/comments.rs`, `ErReview.feedback`, `ErFeedback`, `CommentRef::Legacy` and
  every arm that matches it (`ai/review.rs`, `ai/comments.rs`,
  `er-desktop/src/snapshot.rs`, `er-tui/src/ui/diff_view.rs`), the
  `.er-feedback.json` timestamp note in `sync.rs`, and the `feedback.json`
  entry in the loader's file list.
- Out of scope: the `synced` flag (P05, P06); `lenient_*` helpers.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: a `Legacy` arm that also handles a live `GithubComment` case through a
  shared pattern (`CommentRef::GitHubComment(_) | CommentRef::Legacy(_)`) loses
  nothing, but each split arm needs a test that still covers the remaining
  variant.
- Size: S.

### P02 Delete legacy config paths and the effort fallback

- Branch: `epic/p02-config-legacy`. Lanes: C. Model: `opus`.
- Depends on: none.
- Scope: `legacy_config_candidates`, `migrate_legacy_config` and the
  session-only legacy fallback in `load_global_config`; the legacy cache target
  in `uninstall.rs`; the `agent.effort` field, its reader in `config/mod.rs`, its
  rows in `config/settings.rs` and `config/desktop_settings.rs`, and the entry in
  `desktop-ui/src/lib/components/settings/settingsSearch.ts`;
  `RETIRED_PRESET_MODELS` and `retire_preset_models`.
- Out of scope: the config split and fail-loud loading (§3.7, phase 3/4); the
  rest of the `[agent]` table.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`, `check-ui`,
  `lint-ui`, `test-ui`, `docs-check` (update `docs/config-reference.md`).
- Risks: `agent_runtime` tests pin the `[agent]` fallback argv
  (`what_the_runtime_resolves_versus_what_card_ai_resolves` and its sibling);
  adjust them only for the removed effort flag, and say so in the PR.
- Size: S.

### P03 Delete the importance table and old tier words

- Branch: `epic/p03-importance-legacy`. Lanes: C. Model: `opus`.
- Depends on: P02 (same lane).
- Scope: `ImportanceConfig`, the `[importance.<repo>]` read and its carry-over
  on write, `RiskRuleTable::from_importance`, and the `foundational`,
  `isolated` and `normal` spellings in `config/risk_rules.rs` and the filter
  tests in `app/filter.rs`. Amend ADR 0048: drop "Reading the old table" and the
  consequence that filters read old tiers. Keep the `importance:` filter prefix
  and the `importance` task kind, which ADR 0048 keeps for their own reasons.
- Out of scope: `ReachLevel::Isolated` (`ai/triage.rs`, `triageCard.ts`);
  importance output as an artifact (phase 3).
- Acceptance: `fmt-check`, `clippy`, `test`, `docs-check`.
- Risks: a saved filter naming an old tier now matches nothing; acceptable under
  "old data: no support".
- Size: S.

### P04 Delete the repo-local storage root

- Branch: `epic/p04-repo-local-root`. Lanes: E, F. Model: `opus`.
- Depends on: P01 (lanes).
- Scope: `ER_REPO_LOCAL`, `use_repo_local_storage`, `ErRoot::RepoLocal` and
  every branch on it (`paths.rs`, `storage.rs`, `export.rs`,
  `app/state/{mod,comments,preload,arena}.rs`, `app/state/review_flow/results.rs`,
  `er-desktop/src/snapshot.rs`, `tests/comment_bucket_repro.rs`). Collapse
  `ErRoot` to one variant or a struct; phase 2 replaces it. Update ADR 0003,
  `docs/config-reference.md` and `crates/er-engine/src/watch/CLAUDE.md`.
- Out of scope: the `v1/` root (P15).
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`, `docs-check`.
- Risks: the watcher's `/.er/` filter exists for the repo-local case; confirm it
  has no other purpose before deleting it.
- Size: M.

### P05 Post GitHub comments when they are created

- Branch: `epic/p05-gh-comment-post-on-create`. Lanes: E, F. Model: `opus`.
- Depends on: P04 (lanes). Decision D3.
- Scope: creating a GitHub comment, thread or reply posts it at once and
  records it in `github-comments.json` only after GitHub returns it, on the
  desktop (`InlineThread.svelte`, `ReplyActionBar.svelte` and their commands) and
  in the TUI. A failed post keeps the composer text and shows the error. Engine
  side: `app/state/github_sync.rs`, `ai/comments.rs`.
- Out of scope: deleting the bulk paths (P06); a TUI pending-review UI (5b,
  unless D3 says otherwise).
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`, `check-ui`,
  `lint-ui`, `test-ui`.
- Risks: gh writes. Duplicate posts on retry; a test must show a retried
  create after a timeout does not post twice, or the plan for that must be
  stated. Network failure now blocks creation where it did not before.
- Size: M.

### P06 Delete the local review draft

- Branch: `epic/p06-delete-local-review-draft`. Lanes: E, F. Model: `opus`.
- Depends on: P05.
- Scope: `submit_github_review`, `push_github_comments`, and the per-thread and
  per-reply push commands once nothing calls them (`commands.rs`, `main.rs`,
  `permissions/app-commands.toml`, `CommentsCard.svelte`); engine
  `push_all_comments_to_github`; TUI `push_comments_as_review`,
  `push_all_comments_to_github` and `ConfirmAction::PushComments`; the `synced`
  field and every "not yet pushed" render path (engine, `snapshot.rs`,
  `types.ts`, `commentVisibility.ts`, `annotationActions.ts`,
  `findingReplies.ts`, TUI `diff_view.rs` and `panel.rs`). Update ADR 0049's
  last consequence, ADR 0015's mention of `submit_github_review`, and the
  `submit_github_review` rule in `crates/er-desktop/agent.md`.
- Out of scope: the pending-review flow itself, which stays as it is.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`, `check-ui`,
  `lint-ui`, `test-ui`, `docs-check`.
- Risks: `synced` spans many sites; if the diff passes about 1,500 lines, split
  the desktop-ui removal into its own PR stacked on this one.
- Size: L.

## Phase 1: ports

Each port is a stack: create the crate, move the engine callers, move the
front-end callers and land the guardrail. Crate rules for every new crate:
`[lints] workspace = true`; no dependency on `er-engine` (that would be a cycle);
builds for the headless `er-mcp`; its own `CLAUDE.md` added to the `DOCS` list
in `scripts/docs-check.sh`; added to the `test` recipe.

### P07 Create er-git

- Branch: `epic/p07-er-git-crate`. Lanes: C. Model: `opus`.
- Depends on: P03 (lane).
- Scope: new `crates/er-git` with the `Git` trait, `GitError`, a CLI
  implementation and a test fake. Move `git/status.rs` and `git/blame.rs` into
  it; `git/diff.rs` stays in the engine until `er-core` (phase 4). Move
  `proc.rs` into `er-git`, since `er-github` and `model_discovery.rs` also use
  it; `er-github` will depend on `er-git`. Leave `pub use` shims at the old
  engine paths so callers compile. Decide how the `Git` handle reaches `App`
  (one `Arc<dyn Git>` passed to every constructor) and record it in the crate's
  `CLAUDE.md`. Move the spawn rules from `crates/er-engine/src/git/CLAUDE.md`
  (`--no-color`, `--no-ext-diff`, subprocess only, ADR 0001).
- Guardrail: add `er::git_spawn` to `er-lint-gate` (a syn check flagging
  `Command::new("git")` outside `crates/er-git`) at error tier, with a
  shrink-only allow-list of the files that spawn `git` today under
  `[workspace.metadata.lint-gate]` (D7). Do not touch
  `clippy-warning-budget.json` or run `just clippy-accept`.
- Acceptance: `fmt-check`, `clippy`, `test` (recipe now includes `er-git`),
  `build-engine-headless`, `docs-check`, and `cargo test -p er-lint-gate` via
  `test-all`.
- Risks: a spawn through a variable (`Command::new(bin)`) evades a literal
  check; the reviewer greps for `"git"` near `Command` as well.
- Size: M (moves count after rename detection).

### P08 Move engine git calls behind the port

- Branch: `epic/p08-er-git-engine`. Lanes: E. Model: `sonnet`.
- Depends on: P06 (lane), P07.
- Scope: every `Command::new("git")` in `app/state/*`, `storage.rs`,
  `sidecar_upload.rs` and `ai/review_md.rs` calls the `Git` trait. Add a
  fake-backed test for at least one refresh path. Remove the migrated files from
  the `er::git_spawn` allow-list.
- Out of scope: arena redesign (phase 3); its git calls move as they are.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: the five `App`/`TabState` field-literal constructors each need the
  handle; a missed one compiles only if it defaults, so no `Default` for it.
- Size: M.

### P09 Move front-end git calls and close the git guardrail

- Branch: `epic/p09-er-git-frontends`. Lanes: F, C. Model: `sonnet`.
- Depends on: P08.
- Scope: git spawns in `er-desktop` (`snapshot.rs`, `commands.rs`, `tabs.rs`,
  `projects.rs`, `main.rs`, `commands/line_commits.rs`) and the engine test
  `comment_bucket_repro.rs`. Remove the P07 shims. The `er::git_spawn`
  allow-list is now empty; delete it.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: desktop git calls made under the `App` lock must stay outside it
  (ADR 0015); moving them is a chance to check, not to change, lock scope.
- Size: M.

### P10 Create er-github

- Branch: `epic/p10-er-github-crate`. Lanes: C. Model: `opus`.
- Depends on: P07.
- Scope: new `crates/er-github` with the `GitHub` trait, `GhError` (§3.3), one
  TTL cache, a test fake, and error classification from exit codes and
  `gh api --include` status lines. Move engine `gh_pr_actions.rs` and
  `github/pending_review.rs` behind it. ADR 0026 stays.
- Guardrail: `er::gh_spawn` (same mechanism as `er::git_spawn`, allowed only in
  `crates/er-github`), at error tier with a shrink-only allow-list (D7).
- Acceptance: `fmt-check`, `clippy`, `test`, `build-engine-headless`,
  `docs-check`, `test-all`.
- Risks: classification from `--include` output needs real `gh` samples for
  404, 401, 403 rate limit, 409 and 422; record them as fixtures.
- Size: M.

### P11 Move github.rs into er-github

- Branch: `epic/p11-er-github-engine`. Lanes: E. Model: `opus`.
- Depends on: P08 (lane), P10.
- Scope: move `github.rs` into `er-github`, split by concern so each file reads
  as a rename, and the gh parts of `pr_resolve.rs`, `gh_stack.rs` and
  `review_queue.rs`. Engine callers (`sync.rs`, `app/state/github_sync.rs`,
  `app/state/mod.rs`, `app/state/preview.rs`) use the trait. Replace
  `contains("404")` with `GhError::NotFound`. Add fake-backed tests for comment
  sync, pending review and merge.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: gh writes. The two caches merge into one; a TTL that differs per call
  today must be kept per key.
- Size: L. If the diff after rename detection passes about 1,500 lines, split
  `pr_resolve`/`gh_stack`/`review_queue` into a stacked PR.

### P12 Move front-end gh calls and close the gh guardrail

- Branch: `epic/p12-er-github-frontends`. Lanes: F, C. Model: `opus`.
- Depends on: P09, P11.
- Scope: gh spawns in `er-desktop` (`commands.rs` `run_gh_pr_*_for_open`,
  `gh_pr_actions.rs`, `pr_cache.rs`, `gh_status_cache.rs`, `github_image.rs`,
  `projects.rs`, `main.rs`) and `er-tui/src/input/mod.rs`. Replace
  `is_gh_review_422` with a `GhError` match. The `er::gh_spawn` allow-list is
  now empty; delete it. (P11 removes the engine files from it as it moves them.)
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: gh writes on merge and branch delete (ADR 0040). Keep `pr_cache`
  semantics until phase 6 moves it to the service.
- Size: M.

### P13 One remote-slug parser

- Branch: `epic/p13-repo-slug`. Lanes: none. Model: `sonnet`.
- Depends on: P11.
- Scope: one `RepoSlug::parse` in `er-github` replacing the parsers in the moved
  `github.rs`, `projects_pins.rs`, `er-desktop/src/projects.rs` and
  `er-mcp/src/projects.rs`, with the union of their tests and the
  `git@github.com:` cases they disagree on.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`, `test-all`.
- Risks: `er-mcp` gains a dependency on `er-github`; confirm it stays headless.
- Size: S.

## Phase 2: store

`er-store` is generic: the `Artifact` trait, keys, envelopes and locks. The
`impl Artifact` for each payload stays beside the type in the engine until
`er-core` exists. The store's root is `<app data>/easy-review/v1/`.

### P14 Create er-store

- Branch: `epic/p14-er-store-crate`. Lanes: C. Model: `opus`.
- Depends on: P10.
- Scope: new `crates/er-store` with `ArtifactStore`, `ArtifactKey<T>`, the
  `Artifact` trait (`KIND`, `VERSION`, `migrate` with an empty registry,
  `JsonSchema` via `schemars`), the envelope (§3.4), tmp+rename writes, version
  checks (an unknown version reads as absent), `StoreError`, and an app-scoped
  key for app records. Move the bucket key types from `storage.rs`. Add the
  registry test: kinds found in a store directory must all be registered. Add
  the D5 ingest mode for agent-written kinds.
- Out of scope: callers; locks and change notifications (P23).
- Acceptance: `fmt-check`, `clippy`, `test` (adds `er-store`),
  `build-engine-headless`, `docs-check`.
- Risks: the envelope and the bare-ingest mode are formats users will keep from
  1.0.0 on; review them as an API.
- Size: M.

### P15 Move to the v1 root

- Branch: `epic/p15-v1-root`. Lanes: E, C. Model: `opus`.
- Depends on: P04, P14. Decision D6.
- Scope: `storage_root()` returns the `v1/` directory, which moves config with
  it. On first launch with an old root present, show one notice naming it and
  the old `config.toml` (the existing `App` notification). `er uninstall` offers to delete the old root.
  Update the config path in `CLAUDE.md`, `crates/er-desktop/agent.md` and
  ADR 0003.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`, `docs-check`.
- Risks: anything with a hand-built path to the old root keeps writing there;
  grep for `easy-review` literals.
- Size: S.

### P16 Comments and notes through the store

- Branch: `epic/p16-annotations-store`. Lanes: E. Model: `opus`.
- Depends on: P06, P11, P15.
- Scope: questions, notes and `github-comments.json` read and write only
  through `ArtifactStore` with one writer each. `PrivateAnnotation` and
  `GithubComment` become distinct types (ADR 0007). `GitHub` write methods
  accept only `GithubComment`, and `promote` is the only conversion. Ids keep
  their `n-`/`q-` prefixes on the wire until phase 5.
- Guardrail: add `er::sidecar_fs` to `er-lint-gate`: any `std::fs` call in a
  file on the `[workspace.metadata.lint-gate]` deny list is an error. The list
  starts with the files this PR migrated (`ai/comments.rs`,
  `app/state/comments.rs`) and only grows.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: persistence. Two front ends writing the same bucket before P23's lock
  lands; keep the existing in-process serialization until then.
- Size: M.

### P17 Session state through the store

- Branch: `epic/p17-session-store`. Lanes: E. Model: `opus`.
- Depends on: P16.
- Scope: `session.json`, `reviewed`, `order.json` and `checklist.json` through
  the store; their modules join the `er::sidecar_fs` deny list.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: `reviewed` is written on every checkmark; measure that the envelope
  write does not slow `toggle_reviewed`.
- Size: M.

### P18 Review artifacts through the store

- Branch: `epic/p18-review-artifacts-store`. Lanes: E. Model: `opus`.
- Depends on: P17.
- Scope: `ai/loader.rs` reads `review.json`, expert files, `arbiter.json`,
  professor and triage output through the store, with `impl Artifact` on each
  type. Agent-written kinds use the D5 ingest mode. `lenient_*` helpers stay.
- Out of scope: projection (P20); arena storage (phase 3).
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: a payload that fails validation was silently skipped before; it must
  still not crash a load.
- Size: M.

### P19 Remaining AI artifacts through the store

- Branch: `epic/p19-ai-artifacts-store`. Lanes: E. Model: `opus`.
- Depends on: P18.
- Scope: tour, diagrams (`flows.json` and siblings), summary, change facts,
  finding responses and accepted provenance. `ai/loader.rs` and these modules
  join the deny list.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Size: M.

### P20 Findings projection for experts and the arbiter

- Branch: `epic/p20-findings-projection`. Lanes: E. Model: `opus`.
- Depends on: P18.
- Scope: experts no longer merge into `review.json`; the findings the user sees
  are a projection computed on read from the general review, expert files and
  the arbiter overlay (ADR 0035). Remove `merge_experts_into_review` and its
  callers.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`.
- Risks: finding order and ids must match what the front ends show today; pin
  them with a golden test before changing the code.
- Size: M.

### P21 Scoped re-review as its own artifact

- Branch: `epic/p21-scoped-review-artifact`. Lanes: E. Model: `opus`.
- Depends on: P20.
- Scope: the validation and scoped re-review passes write their own artifact
  (`validation.json`), the projection reads it, and `ai/scoped_merge.rs` with
  `review.prev.json` goes. Prompt changes in `ai/prompts.rs` follow ADR 0023.
- Out of scope: the arena import rewrite (phase 3).
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`, `docs-check`.
- Risks: a prompt change alters agent output; run one real review before merge
  and record it in the PR.
- Size: M.

### P22 App records through the store

- Branch: `epic/p22-app-records-store`. Lanes: F. Model: `opus`.
- Depends on: P13, P15.
- Scope: projects (one writer, moved from `~/.config/er/projects.json` into the
  store), inbox state, saved PRs, the update check and desktop tab persistence
  (`er-desktop/src/{projects,inbox,pr_cache,persist}.rs`,
  `er-engine/src/projects_pins.rs`, `er-mcp/src/projects.rs`). These stay in
  their crates until phase 6.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-desktop`, `test-all`.
- Risks: MCP reads projects; its tool description names the old path.
- Size: M.

### P23 Cross-process locks and change notifications

- Branch: `epic/p23-store-locks-watch`. Lanes: C. Model: `opus`.
- Depends on: P14.
- Scope: an OS advisory lock on `<bucket>/.lock` for every write, a lease-lock
  API for workflows (used in phase 3), and a change channel fed by the existing
  watcher. Nothing consumes the channel yet; mtime polling stays until phase 4.
- Acceptance: `fmt-check`, `clippy`, `test`, `build-engine-headless`.
- Risks: process control. `flock` and `LockFileEx` differ on stale locks after
  a crash; test both a held lock and a dead holder.
- Size: M.

### P24 MCP uploads through the store

- Branch: `epic/p24-mcp-upload-store`. Lanes: none. Model: `opus`.
- Depends on: P19, P23.
- Scope: `sidecar_upload.rs` and `diagram_upload.rs` commit through the store.
  MCP output specs come from the `schemars` derive for every kind, and
  `sidecar_specs.rs` is deleted. Both files join the deny list.
- Acceptance: `fmt-check`, `clippy`, `test`, `test-all`.
- Size: M.

### Order of work

```
lane C:  P02 → P03 → P07 → P10 → P14 → P23
lane E+F: P01 → P04 → P05 → P06
lane E:  P08 → P11 → P16 → P17 → P18 → P19, P20 → P21
lane F+C: P09 → P12          lane F: P22
free:    P13, P24
```

## Phases 3 to 6: milestones

### Phase 3: jobs and workflows

- Goal: `er-agent` with `JobSpec` and `JobRunner` owning slot, argv, spawn,
  decoding, progress and output validation; the host commits outputs; card AI,
  then background, then tab-local move onto it; the review flow and the arena
  become one workflow engine with a `Panel` stage and voting and merge rules in
  pure code.
- Exit criteria: one spawn path, one decoder, one kill path; the slot cap holds
  by construction (ADR 0021); no agent writes into a bucket directly, so the D5
  ingest mode is deleted; importance output is a `RiskRules` artifact; the arena
  adapter, registry, retry loop, `progress.jsonl` and import rewrite are gone;
  `er::agent_spawn` allows the agent CLI spawn only in `er-agent`.
- Known risks: external agents and skills that write sidecars directly must move
  to MCP upload; workflow state needs a new record (claim 6); a Deep run spans
  two job systems until the arena moves.
- Open questions: where `agents.toml` and the provider catalog live; whether the
  panel round count is measured before or after the rebuild (§8); how a
  workflow interrupted by a crash resumes per stage.

### Phase 4: core and service

- Goal: extract `er-core`; build the `Workspace` actor around the desktop's use
  cases; move use cases one command at a time; move view state into the front
  ends.
- Build flag: milestone item M4.1, the first PR of the phase, adds the Cargo
  feature `v1-service` to `crates/er-desktop/Cargo.toml` (optional
  `dep:er-service`) and, for 5b, to `crates/er-tui/Cargo.toml`. desktop-ui
  reads `import.meta.env.VITE_ER_V1_SERVICE`, set by `scripts/tauri-dev.sh` when
  the feature is on. The same PR makes `er-lint-gate` also lint with the
  feature on, adds a `just test-v1` recipe and a CI step running it, because
  the default clippy run and `cargo test` would never compile the new path. The
  flag is off by default and is deleted, with the old path, at the end of
  phase 5 (desktop) and 5b (TUI).
- Exit criteria: `er-core` has no IO, enforced by `crates/er-core/clippy.toml`
  (`disallowed_methods`/`disallowed_types` for `std::process`, `std::fs`,
  `std::thread`, `std::sync::mpsc`, plus the five root keys); no
  `comment_*_override` fields; the settings schema replaces
  `DesktopSettingsSnapshot` at the engine root; the desktop runs on the service
  with the flag on.
- Known risks: the phase touches every hot file; the flag doubles test time.
- Open questions: how session restore splits between front-end files and the
  service; whether the TUI keeps compiling against the old `App` until 5b.

### Phase 5: desktop protocol

- Goal: `er-protocol` with generated TypeScript, versioned resources and
  content-addressed hunks; desktop-ui ported store by store; `snapshot.rs` and
  `commands.rs` deleted.
- Exit criteria: no command returns a `*Snapshot` type and none returns
  `Result<_, String>`, enforced by an `er-lint-gate` syn rule over
  `#[tauri::command]` functions; `types.ts` is generated; one new ADR supersedes
  ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034.
- Known risks: the optimistic layer encodes backend rules; removing it shows
  latency the old path hid.
- Open questions: `tauri-specta` or `ts-rs` (D9); resource granularity for the
  sidebar.

### Phase 5b: TUI

- Goal: the TUI runs on the service with its own `TuiView`.
- Exit criteria: no TUI code mutates domain state directly; the TUI flag and
  old path are deleted; TUI review submission restored if D3 deferred it.
- Known risks: key handling that relied on `App` fields.
- Open questions: none known yet.

### Phase 6: consolidate

- Goal: inbox and projects in the service; MCP on the service; CLI subcommands;
  superseding ADRs.
- Exit criteria: front ends depend only on `er-protocol` and `er-service`,
  checked by an `er-lint-gate` rule over `cargo metadata` (no `deny.toml` exists);
  one `now_epoch_ms`; `CONTEXT.md` changed only where a term changed.
- Known risks: MCP behaviour changes visible to installed skills.
- Open questions: which CLI subcommands ship in 1.0.0.

## Owner decisions

Each has a recommended default. Planning proceeds on the default until answered.

- **D1 Browser annotations.** Deleting the v2 modules would discard the
  browser-rework work stranded on `release/v0.6.1` (claim 1). Default: no
  phase 0 deletion. The owner decides whether `release/v0.6.1` is merged into
  `main` and the browser epic resumes. At the phase 2 gate, if
  `release/v1.0.0` still has no caller, delete the modules then; if it has one,
  `browser-annotations.json` becomes an `Artifact` in phase 3.
- **D2 Parallel release line.** The resolver's `sort -V` always picks
  `release/v1.0.0`, so any `release/v0.6.5` would be skipped. Default: keep a
  v0.6.x line on `release/v0.6.5`; edit the resolver in `CLAUDE.md` on `main`
  to match `release/v0.*` only; epic sessions take their branch from STATUS.
  Merge `main` into `release/v1.0.0` at every phase gate and after every
  v0.6.x release. v0.6.x features reach the epic only through `main`.
- **D3 TUI posting to GitHub.** After P05 and P06 the TUI can post single
  comments but cannot submit a review with a decision. Default: accept that
  until phase 5b and say so in the release notes. Alternative: add a TUI
  pending-review path over `github/pending_review.rs` to the P05 stack (M).
- **D4 Finding and Confidence item in §5.** Default: drop it from the deletion
  list; claim 2 shows the code is live. Nothing is deleted.
- **D5 Agent-written artifacts in phase 2.** Default: the store reads
  agent-written kinds as a bare payload, validated against the type and
  `diff_hash`, until phase 3 commits outputs on the host. Host-written kinds use
  the envelope from P16 on.
- **D6 Config under the v1 root.** Default: config moves with the root, as §5
  implies, and the user re-enters settings once.
- **D7 Seeding the spawn guardrails.** ADR 0038's warn tier exists for a rule
  with many existing sites, and P07 and P10 seed the budget, which is `{}`
  today. `CLAUDE.md` forbids growing a baseline to get green. Default: seed,
  because the seed is a new rule's starting count and each later PR may only
  lower it. Alternative: land each rule at error tier in the PR that reaches
  zero (P09, P12) and check counts by review until then.
- **D8 `origin/chore/remove-dead-agent-runtime`.** Default: close it unmerged;
  the proposal builds on `agent_runtime`.
- **D9 Codegen.** It does not affect phases 0 to 2: `schemars` in P14 is
  independent of both options. Default: decide at the phase 5 gate.

## Risks across the epic

- **Divergence.** Hot files are rewritten here while v0.6.x keeps changing
  them. Without D2's merge cadence, phase 4 starts with large conflicts.
- **Stranded work.** `release/v0.6.1` shows how a release branch can be left
  unmerged; check at each gate that every `release/v*` is merged into `main`.
- **gh writes.** P05, P06, P11 and P12 change how comments, reviews and merges
  reach GitHub. Each needs a fake-backed test and one real run recorded in the
  PR.
- **Formats kept from 1.0.0 on.** The envelope and key layout from P14 are what
  users will keep. Changing them after release needs a migration.
- **Guardrail gaps.** Literal-matching syn rules miss indirect spawns and
  aliased `fs` imports; reviewers grep as well.
- **Partial store migration.** Between P16 and P24 some artifacts use the store
  and some do not. Two writers on one bucket are possible until P23.
- **Machine load.** Every heavy check takes one machine-wide lock; three lanes
  is the ceiling, and two is the common case.

## Re-planning at a phase gate

1. `git fetch origin`; confirm every PR of the finished phase is merged and
   STATUS matches.
2. Merge `main` into `release/v1.0.0` (D2) and run `just ci` through the lock.
3. Re-check each claim the next phase rests on against the merged tree, as in
   "Verified starting point". Record what changed.
4. Answer the phase's open questions, with the owner where they are decisions.
5. Write the next phase's PR entries in an addendum in this folder, in the
   format above, with lanes re-derived from the hot files that phase touches.
6. Log the gate in BUILD_LOG.md.
