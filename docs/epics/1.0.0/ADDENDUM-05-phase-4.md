# Addendum 05: phase 4, core and service

Drafted ahead of the phase-4 gate (#397), following [PLAN.md "Re-planning at a
phase gate"](./PLAN.md#re-planning-at-a-phase-gate). Where this file disagrees
with PLAN.md or an issue body, this file wins once committed;
[ADDENDUM-01](./ADDENDUM-01-decisions.md) and
[ADDENDUM-02](./ADDENDUM-02-issue-decisions.md) (D11–D22) still bind.

Claims were checked against `release/v1.0.0` at `60c9571ff` on 2026-11-11,
before P28b–P37 merged. This copy re-checks every citation by symbol against
`release/v1.0.0` at `afd78bced` (2026-11-14), after phase 3 closed with #520,
#522 and #524 merged. Only citations and facts phase 3 made wrong are changed;
the decisions, scope and PR entries are otherwise the r4-final text. No marker
remains: all sites previously flagged for #520 and #524 are resolved at this
head. Amendment-by-amendment changes are listed at the end.

## Re-check changes (this copy)

Method: every citation below was re-found by symbol with `git grep`/`git show`
at `47ca312f7`; counts are fresh at that ref.

**Citations moved (line numbers updated in place):**

- `pub struct App` `:5842`→`:5839`; struct end `:5974` unchanged. `TabState`
  `:680`→`:679`; `mod.rs` 17,523→17,515 lines; `comments.rs` 7,191→8,682.
- `App` fields: `pending_ai_selection_override` `:5901`→`:5900`,
  `pending_background_tasks` `:5921`→`:5920` (type `:5921`),
  `review_flow_run_targets` `:5947`→`:5948` (type `:5949`), `input_mode`
  `:5851`→`:5850`, `should_quit` `:5854`→`:5853`, `overlay` `:5857`→`:5856`,
  `remote_url_input` `:5883`→`:5882`, `ai_poll_counter` `:5880`→`:5879`,
  `pending_hub_action` `:5904`→`:5903`, `last_terminal_width`
  `:5907`→`:5906`, `panels_visible` `:5910`→`:5909`.
- `TabState` fields: `selected_file` `:697`, `current_hunk` `:700`,
  `current_line` `:703`, `diff_scroll` `:709`, `h_scroll` `:712`,
  `h_scroll_old` `:718`, `h_scroll_new` `:721`, `panel` `:727`,
  `panel_scroll` `:730`, `panel_focus` `:733`, `file_tree_width` `:736`,
  `panel_width` `:739`, `review_cursor` `:754`, `filter_input` `:826`,
  `comment_textarea` `:833`; browser block `:1098`–`:1106`.
- Overrides: declarations `mod.rs:862`, `:867`, `:881`; take/clear sites
  `comments.rs:887`, `:892`–`:893`, `:1169`, `:1224`–`:1225`, `:1254`,
  `:1321`–`:1322`, `:1351`, `:1400`–`:1401`, `:1642`–`:1643`; constructor
  blocks `mod.rs:1616`–`:1620`, `:1760`–`:1764`, `:1898`–`:1902`,
  `:2054`–`:2058`, `:10054`–`:10058`; desktop id setters
  `commands.rs:2514`, `:2560`, `:9509`.
- Engine gates: `lib.rs` `app` `:7` (cfg `:6`), `arena` `:9` (cfg `:8`),
  `export` `:20` (cfg `:19`); config re-exports `:11`–`:14`.
- Review types: `Confidence` `:245`, `Finding` `:316`, `AiState` `:878`,
  `impl Artifact for ErTour` `:623`; `ErReview` `:70` and
  `impl Artifact for ErReview` `:86` unchanged. Annotation impls
  `comments.rs:406`, `:417`, `:428`.
- Risk evaluation `risk_rules.rs:168`, `:249`, `:273`, `:279`; the
  `RiskRuleTable`/`RiskRules` block runs `:154`–`:336`.
- `ProjectionInputs` `:30`–`:58`; `FindingsProjection` `:49`;
  `build_findings_projection` `:57`; file 649 lines.
- `arena/voting.rs` 460 lines; `merge.rs` 196, `identity.rs` 35,
  `provenance.rs` 242 unchanged.
- Store: `Artifact` `artifact.rs:12`; `migrate` `:27`; `type Slot: SlotName`
  `artifact.rs:18`; `SlotName` `key.rs:149` (impls `:162`, `:205`, `:236`);
  `ArtifactKey` `key.rs:271`; `Scope` `:125` with `Repo` `:135`; `BranchView`
  `:95` and `as_str`/`parse` `:102`–`:120` unchanged.
- Desktop: 199 `#[tauri::command]` functions; `commands.rs` 155 (14,136
  lines), `arena_commands.rs` 20 (`rerun_review_flow` added), `config_commands.rs`
  11; `snapshot.rs` 8,196 (builder `build_snapshot_with_delta` `:3079`);
  `inbox.rs` 1,070 (`InboxState` `:168`); `projects.rs` 1,358 (record block
  `:16`–`:616`); `tabs.rs` 693 (`TabsFile` `:51`, `save_tabs` `:70`,
  `save_app_tabs` `:81`, `persist_app_tabs` `:107`, `load_tabs` `:115`,
  persistence `:51`–`:121`); `pr_cache.rs` 804; `gh_status_cache.rs` 445;
  `pr_open_cache.rs` 266; `remote_pr_open_cache.rs` 276; `auto_triage.rs`
  375. `AppState` `commands.rs:155`, `Arc<Mutex<App>>` `:157` unchanged.
  `PrOpenFreshness` is `commands.rs:230`; P45's ":118" is the
  `pr_open_cache.rs` import of it, now `:116`. The desktop composition lives in
  `er-desktop/src/store_registry.rs` (#524).
- Config: `apply_config_field` `desktop_settings.rs:231`; the snapshot
  `:110`–`:129` (`risk_*` `:119`/`:121`/`:125`, `importance_*`
  `:126`/`:127`/`:128`) and `desktop_settings_snapshot :174` unchanged by
  P37's split.
- Session: `session_store.rs` moved to `crates/er-engine/src/session_store.rs`
  (#524), where `SessionState` is `:18` and its view fields are
  `:28`/`:32`/`:36`/`:40`/`:44`; `capture_session` `mod.rs:5648`,
  `restore_session` `:5681`, `save_session` `:5762`; TUI call sites
  `main.rs:315`, `:675`, `:701` unchanged.
- Notes: P42's `github_sync.rs` does not exist; the always-compiled core is
  `sync.rs`.
- **#520 and #524 resolved** at the merged head `afd78bced`: the config
  paragraph's lines are unchanged by P37's split, `config/agents.rs` exists
  (657 lines) and `config/mod.rs` loads and saves `agents.toml` beside
  `config.toml`; P44 depends on the merged P37; #524's session move and missing
  desktop composition are in, which is why the `mod.rs` anchors above sit two
  lines lower than they did at `47ca312f7` and the fifth constructor is
  `:10054`.
- `er-lint-gate` clippy args `main.rs:353`–`:356`; CI test job
  `ci.yml:78` unchanged.
- IO counts: engine 701 `std::fs`, 52 `std::process`, 78 `std::thread`,
  44 `std::sync::mpsc`; `app/state` 332/34/44/39. `loader.rs` has 26
  `std::fs` occurrences, not 62.

**Facts that phase 3 made wrong (corrected in place):**

- `App` has 37 field declarations now (22 `pub`, 6 `pub(crate)`, 9 private):
  P32a removed `review_flow_leases`. The struct carries `workflow_leases`
  (a `HashMap<String, er_store::LeaseGuard>`) at `:5938` instead.
- `Mode` no longer exists anywhere in `crates/`. P30b.2 deleted the enum
  outright; the `Artifact` move takes `SlotName` and its slot types only.
- `merge_into_global_config` is gone (P31); `RiskRulesArtifact`
  (`risk_rules.rs:339`, `load :355`) exists and is registered.
- `accepted_provenance` no longer exists. P34b replaced it with
  `ai/accepted_findings.rs` (172 lines, no IO); `projection.rs` imports
  `AcceptedFinding` at `:21` and restores provenance from that input in a
  local `apply_accepted_provenance` at `:242`. The projection's tests no
  longer import `crate::arena`.
- The goldens are 11 JSON files (cases 01–10; case 05 is split a/b), not
  eight. P34a added cases 09 and 10.
- `lenient_verdicts` is defined in `review.rs:175` and only referenced from
  `arbiter.rs:24`; it already moves with `review.rs` in P39.1.
- `arena/identity.rs` imports neither `arena/model.rs` nor
  `arena/schema.rs`; only voting, merge and provenance do.
- `inbox.rs` imports `er_store` directly, not the engine's `storage` and
  `store_registry` modules; the projects record block is now
  `projects.rs:16`–`:616`, not `:22`–`:629`.
- P33b landed: `arena/registry.rs` is gone; `lib.rs:7` is `pub mod app;`.
- The `comments.rs` figures moved by +62 to +84 for the override take sites,
  reflecting P29/P30b growth.

**Premises that no longer hold — decisions applied (coordinator):**

1. **P48/D22 already shipped; the entry is dropped from phase 4.** PR #503
   (`8460a7e22`) retired the auto-triage flag in phase 3, and its own tests
   cover it. The P48 entry is replaced by one line below. Leave
   `auto_triage_max_diff_kb` alone.
2. **P39.3's `accepted_provenance` clause is dropped with no replacement.**
   P34b deleted the module and the projection takes `accepted_findings` with no
   IO.
3. **`Mode` is a correction:** the move list is `SlotName` and its slot types
   only.
4. **P47.1 names `App::next_unreviewed_file`** (`mod.rs:9551`) in the
   navigation extraction, beside the `navigation.rs` functions.

## Claims (re-checked at `afd78bced`)

- **The god structs.** `App` has 37 field declarations (`mod.rs:5839`–`:5974`):
  22 `pub`, 6 `pub(crate)`, 9 private, including
  `pending_ai_selection_override` `:5900`. Method: a brace-depth scan from
  `pub struct App {` to its matching `}`, counting declarations at depth 1
  that match visibility + name + `:`; two declarations put their type on the
  next line (`pending_background_tasks:5920`, `review_flow_run_targets:5948`)
  and a single-line count misses them. P32a removed `review_flow_leases`;
  `workflow_leases` (a `LeaseGuard` map) sits at `:5938`. `TabState` has 115
  public fields (`:679`). `app/state/mod.rs` is 17,515 lines;
  `app/state/comments.rs` is 8,682.
- **View state lives in the domain types.** `TabState`: `selected_file` `:697`,
  `current_hunk` `:700`, `current_line` `:703`, `diff_scroll` `:709`,
  `h_scroll` `:712`, `h_scroll_old` `:718`, `h_scroll_new` `:721`, `panel`
  `:727`, `panel_scroll` `:730`, `panel_focus` `:733`, `file_tree_width`
  `:736`, `panel_width` `:739`, `review_cursor` `:754`, `filter_input` `:826`,
  `comment_textarea` `:833`, browser fields `:1098`–`:1106`. `App`:
  `input_mode` `:5850`, `should_quit` `:5853`, `overlay` `:5856`,
  `remote_url_input` `:5882`, `ai_poll_counter` `:5879`,
  `pending_hub_action` `:5903`, `last_terminal_width` `:5906`,
  `panels_visible` `:5909`.
  Word hits at this tree (`git grep -o -w`; test modules included, so they
  count vocabulary: `overlay` includes the AI overlays, and `browser_*` in
  `er-desktop` includes the `browser_proxy`/`browser_webview` module paths):
  `selected_file` 146 engine / 17 TUI / 6 desktop; `current_hunk` 151/2/8;
  `current_line` 149/9/2; `overlay` 354/62/44; `diff_scroll` 81/17/2;
  `input_mode` 45/60/6. The engine's `app/state/navigation.rs` is 1,298 lines
  of view logic both front ends call. The browser fields are defined on
  `TabState` but have no engine reader outside `mod.rs` (definitions and
  constructor literals); the `er-desktop` hits include the browser modules,
  and the field reads are in the desktop's snapshot and commands.
- **Three `comment_*_override` fields remain** (`mod.rs:862`, `:867`, `:881`),
  read or taken at `comments.rs:887`, `:892`–`:893`, `:1169`, `:1224`–`:1225`,
  `:1254`, `:1321`–`:1322`, `:1351`, `:1400`–`:1401`, `:1642`–`:1643`.
- **Engine UI coupling.** `ui = ["dep:tui-textarea-2"]`
  (`crates/er-engine/Cargo.toml:14`); `app` and `arena` are `ui`-gated
  (`lib.rs:6`/`:7`, `:8`/`:9`), as is `export` (`:19`/`:20`). `sync.rs` is
  always compiled and must not depend on a feature-gated module (`sync.rs:1`–`:16`).
- **Pure code and its real closure.**
  - `git/diff.rs` has no process or fs call, but imports `er_git::FileStatus`
    (`diff.rs:1`; `er-git/src/status.rs:42`), so moving it as is pulls an
    adapter crate into the core.
  - `ai/review.rs` has no production fs/process (`ErReview:70`,
    `Confidence:245`, `Finding:316`; two `std::fs::write` calls at `:1828`
    and `:1853` are inside `#[cfg(test)]`); `ai/comments.rs`
    (`ErQuestions:190`, `ErNotes:206`, `ErGithubComments:314`).
  - `config/risk_rules.rs:168`, `:249`, `:273`, `:279` is the evaluation
    side; `merge_into_global_config` is gone (P31), replaced by the
    `RiskRulesArtifact` writer (`:339`).
  - The branch helpers are not standalone: `storage_branches_match`
    (`ai/loader.rs:119`) calls `crate::storage::slug_branch`
    (`storage.rs:278`); `loader.rs` has 26 `std::fs` occurrences.
  - The projection (`ai/projection.rs:30`–`:58`, 649 lines, no fs/process in
    the file) imports `accepted_findings::AcceptedFinding` (`:21`; the old
    `accepted_provenance` module is gone with P34b), and the merge functions
    from `arbiter`, `experts` and `professor`, each mixed with its `load_*`
    reader.
  - The arena rules (`arena/voting.rs` 460, `merge.rs` 196, `identity.rs` 35,
    `provenance.rs` 242 lines; no IO in the four files) import
    `arena/model.rs` and `arena/schema.rs` (identity imports neither), and
    `provenance.rs:4` needs `expert_by_id`/`lens_for_producer_id`
    (`ai/experts.rs:112`, `:223`).
- **IO is spread through the engine.** Grep hits in `crates/er-engine/src`:
  701 `std::fs`, 52 `std::process`, 78 `std::thread`, 44 `std::sync::mpsc`;
  `app/state` alone holds 332/34/44/39.
- **Store types.** `Artifact` is `er-store/src/artifact.rs:12`; its `migrate`
  returns `StoreError` (`:27`), which holds `std::io::Error`, `HolderInfo`,
  `PathBuf` and `MAX_ARTIFACT_BYTES` (`error.rs:1`–`:4`), and it has
  `type Slot: SlotName` (`artifact.rs:18`; the `SlotName` trait is
  `key.rs:149`, impls for `()`, `ExpertId`, `DiagramId` at `:162`, `:205`,
  `:236`). `Mode` was deleted in P30b.2. `BranchView` `key.rs:95` has no
  serde derive: it round-trips through `as_str`/`parse` (`:102`–`:120`), and
  its engine users are test modules in `storage.rs:912` and
  `watch/mod.rs:169`. `Scope` `:125` (with P31's `Scope::Repo` `:135`);
  `ArtifactKey` `:271`. `er-store` has no `er-*` dependency.
- **Desktop.** The crate has 199 `#[tauri::command]` functions:
  `commands.rs` 155 (14,136 lines), `arena_commands.rs` 20,
  `config_commands.rs` 11, and 13 across the remaining modules. `AppState`
  (`commands.rs:155`) holds `Arc<Mutex<App>>` `:157` plus caches and
  in-flight maps; `snapshot.rs` is 8,196. Domain logic in the crate: inbox
  (`inbox.rs:16`, `:65`, `InboxState:168`, 1,070 lines; imports `er_store`
  directly), projects (`projects.rs:16`–`:616`, 1,358), tab descriptors
  (`tabs.rs:25`–`:116`, 693), caches (`pr_cache.rs` 804,
  `gh_status_cache.rs` 445, `pr_open_cache.rs` 266,
  `remote_pr_open_cache.rs` 276 in-memory), auto-triage (`auto_triage.rs` 375;
  the project flag is already retired — see P48). `persist.rs` is gone (P22).
- **Config surface.** `DesktopSettingsSnapshot`
  (`config/desktop_settings.rs:110`) carries the field lists plus
  `risk_rules`/`risk_default`/`risk_files` (`:119`/`:121`/`:125`, with
  `risk_repo` at `:117`) and the legacy
  `importance_rules`/`importance_default`/`importance_files`
  (`:126`–`:128`); `ConfigHubFieldDto` `:22`; `desktop_settings_snapshot`
  `:174`; `apply_config_field` `:231`; all re-exported at the engine root
  (`lib.rs:11`–`:14`). No `SettingsSchema` exists anywhere in the tree. The
  hand-declared label/description table is `settings_fields_grouped`
  (`config/settings.rs:60`). P37's split is in place (`config/agents.rs`,
  657 lines; `config/mod.rs` loads and saves `agents.toml` beside
  `config.toml`) and these lines are unchanged by it.
- **Session restore is TUI-only.** `SessionState` holds view fields
  (`crates/er-engine/src/session_store.rs:18`: `selected_file:28`,
  `current_hunk:32`, `current_line:36`, `diff_scroll:40`, `h_scroll:44`).
  `save_session`/`restore_session` are called from `er-tui/src/main.rs:315`,
  `:675`, `:701` and tests; no desktop code calls them. `capture_session`
  `mod.rs:5648`, `restore_session` `:5681`, `save_session` `:5762`.
- **No gate-override domain field exists.** A search for `gate_override` or
  "gate override" across engine, desktop and TUI finds nothing; the proposal
  names the idea, the model has no field.
- **The build flag does not exist.** `scripts/tauri-dev.sh` knows only `--logs`
  and the dev port; no `VITE_ER_V1_SERVICE`. desktop-ui reads
  `VITE_ER_LOG`/`VITE_ER_DESKTOP_PROFILE_POLL` (`profileLog.ts:22`, `:26`). No
  `test-v1` in the `justfile`. `er-lint-gate` runs
  `cargo clippy --workspace --all-targets` with default features only
  (`crates/er-lint-gate/src/main.rs:350`–`:358`); CI's Test job runs
  `cargo test` (`.github/workflows/ci.yml:78`) and already path-filters jobs via
  its `changes` output.
- **Front-end coupling.** The desktop owns `Arc<Mutex<App>>`
  (`commands.rs:157`); the TUI drives `App` directly through
  `er-tui/src/input/**` and has no library target. `er-mcp` builds the engine
  headless (`crates/er-mcp/Cargo.toml:15`).
- **The effect pattern exists.** `sync.rs:1`–`:16` documents the
  snapshot → fetch off-lock → apply shape the actor adopts.

## Phase-3 changes, as landed (release head `37b4fa0f7`)

- **P28b** landed: the `review_flow.rs` launch and the named runner and arena
  progress channels that P43's `JobProgress` events wrap are in place.
- **P29** deleted `comments.rs`'s remaining spawn paths and the summary
  command.
- **P30a** moved job outputs onto the store; **P30b.1** converted
  `comments.rs` and `review_flow_output.rs` reads; **P30b.2** deleted
  `Mode::Bare` and then `Mode` itself, so the `Artifact` trait P39 moves has
  one storage shape and no `Mode` to carry.
- **P31** added `Scope::Repo` (`key.rs:135`) and the `RiskRules` artifact,
  and deleted `merge_into_global_config`. P44 still deletes the
  `importance_*` snapshot DTOs (`desktop_settings.rs:126`–`:128`).
- **P32a** removed `App.review_flow_leases` (`workflow_leases` is its
  replacement), so `App` has 37 fields; **P32b** added the `Panel` stage and
  `PanelRun`; **P33a/P33b** moved arena calls onto the runner and deleted the
  registry, `progress.jsonl` and the glue (`arena/registry.rs` is gone;
  `lib.rs:7` is `pub mod app;`).
- **P34a/P34b** added goldens 09 and 10, deleted the coverage of the arena
  import, and replaced `accepted_provenance` with `accepted_findings`, which
  has no IO. P39.3's P34b dependency is satisfied at this ref.
- **P36** merged at `afd78bced`: `session_store.rs` is at the crate root
  (`:18`), the five desktop records live in `er-desktop/src/store_registry.rs`,
  and the audit and `er::registry` rule are in.
- **P37** merged at `738919af5`: `config/agents.rs` (657 lines) holds the
  agent settings, `config/mod.rs` loads and saves `agents.toml` beside
  `config.toml`, and the settings schema (P44) sits on the split.
- **P48's flag retirement shipped in phase 3** as PR #503 (`8460a7e22`), which
  is why the P48 entry below is one line: the flag, its commands,
  `should_queue_auto_triage` and the settings toggle are gone, and the
  first-launch notice exists.

## Decisions settled by the coordinator

- **Crate graph (B1).** Use-case functions live in the engine's always-compiled
  half, in `sync.rs`'s shape ("A headless consumer calls these directly",
  `sync.rs:7`); the flag-off `App` calls them. `er-service` depends on
  `er-engine` with `default-features = false` and owns the actor. No cycle and
  no duplication. `er-mcp` already builds the engine that way
  (`crates/er-mcp/Cargo.toml:15`), which is what proves the non-`ui` half
  carries storage, the store registry, the loaders, config and `sync.rs`. The
  use cases move up into `er-service` when the old path is deleted in phases 5
  and 5b. This is the review's option (d). Option (a) has the same dependency
  edge but moves the use cases into the service up front, which needs a new
  entry before P40; option (b) would move `store_registry`, the storage paths,
  the loaders and config IO down first, also a new entry; option (c)
  duplicates every use case. Code both paths call lives in `er-engine`'s
  non-`ui` half; `er-service` holds only the actor, its commands and its read
  path until phases 5 and 5b. P40 records this graph in an ADR and adds its
  edges to the risks.
- **Workspace (A3).** One actor per repo or worktree, owning that repo's tabs;
  remote-only PR tabs belong to a per-repo-slug workspace. P40 adds **Workspace**
  to `CONTEXT.md` and moves "workspace" out of Tab's _Avoid_ list
  (`CONTEXT.md:43`).
- **`Artifact` move (A4, A13).** The move is forced by the orphan rule:
  `ErReview`, `ErTour`, `ErQuestions`, `ErNotes`, `ErGithubComments` and
  `ArbiterReview` implement `er_store::Artifact` today (`ai/review.rs:86`,
  `:623`; `ai/comments.rs:406`, `:417`, `:428`; `ai/arbiter.rs:65`). Once
  those types live in `er-core`, the impls must live in `er-core` or
  `er-store`. Moving the trait to `er-core` is the chosen model; it takes
  `SlotName` and its slot types (the `Mode` enum no longer exists, P30b.2),
  and a core-side migration error with `StoreError: From<MigrateError>` in the
  store (`StoreError` cannot move). The design review checks this as an API
  change. `BranchView` does not move: no core type needs it yet, and it has
  no serde derive.
- **No-IO enforcement (A6).** A per-crate `clippy.toml` cannot ban a module
  path, so P39 adds an `er-lint-gate` syn rule, `er::core_io`, rejecting any
  `std::fs`, `std::process`, `std::thread` or `std::sync::mpsc` path under
  `crates/er-core`, plus a dependency allow-list check on `er-core`'s
  `[dependencies]`.
- **desktop-ui on the flag path (A8).** In phase 4 the flag-on backend builds
  the same snapshot DTOs from service state, and service `Event`s bump the
  existing revision (ADR 0011). desktop-ui changes only where a command's
  parameters change (P41's explicit tab and anchor, P44's settings). The
  resource protocol and the store-by-store port stay in phase 5.
- **Routing spread (A9).** Each use-case entry routes its own desktop commands
  on the flag path and lists them; P47 keeps the open, diff and file commands
  and the exit check. Navigation and view-only commands stay in the front end.
- **`test-v1` CI scope (A13).** The job is gated on the existing `rust` and `ui`
  change outputs, and `test-v1` joins `just ci`, which the phase gate runs.
  Pushes already run everything.
- **Settings snapshot placement (A13).** "Replaces `DesktopSettingsSnapshot` at
  the engine root" is met by moving the type out of `er-engine` into
  `er-desktop`, where phase 5 deletes it with `snapshot.rs`. No schema-backed
  adapter for the flag-off path is needed; the flag-off path keeps the type it
  has today.
- **Session restore (A15).** Only the TUI saves and restores `SessionState`;
  every field except `diff_hash` and `branch` is TUI view state or the TUI's
  comment draft, and `comment_draft_client_id` is retry-dedupe state from #403
  that must survive wherever the draft goes. `SessionState` becomes the TUI's
  view file in 5b; the desktop keeps its own view persistence. There is no
  behaviour change in phase 4. `SessionState` now lives in
  `crates/er-engine/src/session_store.rs` (#524).
- **Latency budget (A14, B6).** No more than 10% or 2 ms above the flag-off
  median, whichever is larger, over N runs, measured on open target, mark
  reviewed and next file. P40 and P47 both carry it in their done-when.
  "Speed is a feature" is the fourth design principle, and P17's 5 ms budget is
  the precedent for the form.

## Owner decisions (decided)

The owner answered all four; they are recorded in ADDENDUM-02 as D19–D22 and
bind this plan.

- **D19 (O1) Tab persistence and the app-scoped desktop state.** Phase 4 moves
  what the Workspace owns: the tab list and tab persistence (`tabs.rs:25`–`:116`,
  with `TabsFile:51`, `save_tabs:70`, `save_app_tabs:81`,
  `persist_app_tabs:107`, `load_tabs:115`–`:121`) and the open-target caches (`pr_open_cache.rs`,
  `remote_pr_open_cache.rs`). Inbox, projects, `pr_cache.rs`,
  `gh_status_cache.rs` and `auto_triage.rs` stay in the desktop until phase 6, as
  PLAN says: they are app-scoped records (`Scope::App`), a per-workspace actor
  would not own them, and the caches import phase-5 deletion targets
  (`PrInfo`, `GhStatusCache`; `pr_open_cache.rs` imports `crate::commands`).
  P45 becomes M.
- **D20 (O2) Phase-4 view-state scope.** Phase 4 moves only state with a single
  front-end owner: `browser_*` (defined on `TabState`, read only by the
  desktop) and `panels_visible`. Selection, cursor, scroll, overlay and input
  mode move in 5b, together with `navigation.rs`.
- **D21 (O3) Flag-on desktop completeness.** The flag-on desktop may stay
  incomplete until P47; `test-v1` covers the ported use cases, and the owner
  does not dogfood the flag until P47. This sets how much interim compatibility
  each entry pays for.
- **D22 (O4) Retire the legacy auto-triage flag.** **Shipped in phase 3 as PR
  #503 (`8460a7e22`)**, so the P48 entry is dropped and #503's tests cover it.
  The project flag and its dead path are gone, `retired_auto_triage_set`
  (`projects_pins.rs:209`) and the first-launch notice (`comments.rs:951`, call
  `:2935`) exist, and the module doc no longer says the flag is kept for
  settings compatibility (`auto_triage.rs:1`–`:4`).
  `auto_triage_max_diff_kb` is a different, live setting and stays.
  Auto-triage still stays in the desktop until phase 6 under D19.

## PR entries

Shared acceptance: `fmt-check`, `clippy`, plus `test`/`test-desktop` as the
touched crates require; entries touching `er-store`, new crates or features add
`test-all`; every entry that changes the flag path also runs `test-v1`.
`er-mcp`'s headless build is checked by `build-mcp` when engine features or
dependencies change. The stop line is 1,500 added-plus-changed lines after
rename detection; pure deletions are measured by what remains and do not count
toward it (ADDENDUM-04's L5).

### P38 (M4.1) Build flag and the `er-service` skeleton

- Branch: `epic/p38-v1-service-flag`. Lanes: C, F. Model: `opus`. Depends on: phase 3 closed at the gate.
- **Goal:** a flag that compiles both paths, and the plumbing that keeps the new path tested.
- **Done when:**
  - `crates/er-service` exists with a minimal `Workspace`/`WorkspaceHandle` skeleton and the crate graph of the decisions above; `er-desktop` and `er-tui` gain `v1-service = ["dep:er-service"]` (optional dependency), off by default.
  - The desktop backend exposes one boot value of the compiled feature; desktop-ui refuses to start when `VITE_ER_V1_SERVICE` and the boot value disagree, and `scripts/tauri-dev.sh` sets the variable when the feature is on.
  - `er-lint-gate` runs a second clippy pass with the feature on for the packages that declare it; a planted warning in feature-only code fails the gate. Check that `--no-stale` and `clippy-prune` handle duplicate locations before relying on the budget file.
  - `just build-service-headless` (`cargo check -p er-service --all-targets`) joins the recipes beside `build-engine-headless`, and `test-v1` and CI run it too; `er-service`'s full dependency set is declared here, so P40 adds no manifest or lockfile change.
  - `just test-v1` runs the flag-on suite; CI adds the job gated on the `rust`/`ui` outputs; `test-v1` joins `just ci`.
  - The flag-off build and tests are unchanged.
- **Planted breaks:** a default-build test asserts `!cfg!(feature = "v1-service")`, so flipping the default fails it; starting desktop-ui with the variable set and the boot value false fails the mismatch test; removing the second clippy pass leaves a feature-only warning unreported; an `er_engine::app::App` reference in `er-service` passes the workspace `cargo test` and fails `build-service-headless`.
- **Fallback split:** P38.1 crate and features; P38.2 gate and CI.
- Size: M.

### P39.1 `er-core`: crate, types and the `Artifact` move

- Branch: `epic/p39a-er-core-types`. Lanes: C, E-core. Model: `opus`. Depends on: P38 (C sequence), P30b.2 (merged at the gate).
- **Goal:** the first core crate, holding the domain types and the store trait.
- **Done when:** `crates/er-core` holds `FileStatus` (moved down; `er-git` re-exports it), the diff model (`git/diff.rs:5`–`:76`), the review and finding types (`ai/review.rs:70`, `:245`, `:316`) and the annotation types (`ai/comments.rs:190`, `:206`, `:314`); the `Artifact` trait, `SlotName` and its slot types and a core migration error move per the decision above; `er-store` depends on `er-core` and re-exports the trait and `SlotName`. `AiState` (`ai/review.rs:878`) stays in the engine because it aggregates loaded state; `relocate.rs`, the type-only halves of `arbiter.rs` (`ArbiterEffect`, `ArbiterVerdict`, `ArbiterRuling`), `ValidationEffect` from `validation.rs` and `ai/identity.rs` move with the review types, because `review.rs` names them at `:5`, `:35`/`:177`/`:909`, `:526`, `:917`. `lenient_verdicts` is defined in `review.rs:175` and moves with the review types. The `Mode` enum no longer exists, so nothing carries it. The non-test imports are named here so the move is not discovered at compile time. Engine modules import from `er-core` and re-export until their use case moves.
- **Planted breaks:** changing one `as_str` string fails the store key-path test; a test asserts `TypeId::of` equality for each re-exported type; an `fs::copy` in the crate fails `er::core_io`; adding `dirs` to `er-core` fails the dependency allow-list.
- Size: L.

### P39.2 `er-core`: risk evaluation, hashes, branch helpers and arena rules

- Branch: `epic/p39b-er-core-rules`. Lanes: C, E-core, W. Model: `opus`. Depends on: P39.1, P31 (merged at the gate).
- **Goal:** the pure rules, with their real dependencies.
- **Done when:** risk evaluation (`config/risk_rules.rs:154`–`:336`) and its path matching (`config/path_rules.rs`, which uses `glob::{MatchOptions, Pattern}` at `:7`), the diff-hash helpers and `slug_branch` (moved out of `storage.rs:278`) live in `er-core`; `glob` joins `er-core`'s dependency allow-list; `arbiter.rs`, `experts.rs` and `professor.rs` split into types plus merge functions (core) and their `load_*` readers (engine); `expert_by_id` and `lens_for_producer_id` move with the expert table; `arena/model.rs`, `arena/schema.rs`, `arena/voting.rs`, `arena/merge.rs`, `arena/identity.rs` and `arena/provenance.rs` move with the rules; `er-git`, `er-github` and `er-store` re-export what moved. `config/risk_rules.rs`, `config/path_rules.rs` and `storage.rs` keep one-line `pub use er_core::…` shims, so `config/mod.rs` and `storage.rs` are not edited beyond that line; the `storage.rs` shim is this entry's only E-svc touch and is a rebase-time addition. The arena files are lane W, so this entry holds W too. `arena/identity.rs` depends on `ai/identity.rs` (`canonical_finding_text`) and `sha1`, not on `arena/model.rs`/`schema.rs`.
- **Planted breaks:** a risk evaluation test fails if the precedence order changes; the `TypeId` equality test covers the moved rules; an `fs::read` in the crate fails `er::core_io`, and a `glob::glob` call fails the same rule (the pattern that `disallowed_methods` cannot express is added to `er::core_io`).
- **Fallback split:** P39.2.1 risk and helpers; P39.2.2 the arena rules and their closure.
- Size: L.

### P39.3 `er-core`: the projection

- Branch: `epic/p39c-er-core-projection`. Lanes: C, E-core. Model: `opus`. Depends on: P39.2, P33b and P34b (both merged at the gate).
- **Goal:** the findings projection in the core, after the import and provenance run reads are gone.
- **Done when:** `ai/projection.rs:30`–`:58` and what it still needs — the `arbiter`/`experts`/`professor` merge functions and the remainder of `validation.rs` that P39.1 left — live in `er-core`; `ai/identity.rs` moved in P39.1 and `arena/identity.rs` in P39.2; the ten golden cases (11 files) under `crates/er-engine/tests/golden/findings_projection/` still pass unchanged.
- **Planted breaks:** a projection that drops the lens backfill fails a golden; a `fs::read` or `crate::arena` reference in the core fails `er::core_io`.
- Size: M.

### P40 Workspace actor

- Branch: `epic/p40-workspace`. Lanes: E-svc. Model: `opus`. Depends on: P38, P39.1, and a design issue that settles in-memory state ownership (A2) before code. Under pair (b), P40 and P41 do not edit `crates/er-core/**` or `ai/**`; any such need waits for P39.3.
- **Goal:** Commands in, Events out, one actor per repo/worktree; the first use case behind the flag.
- **State ownership (A2).** The design issue chooses (a) the actor owns `App`, (b) both exist and `App` reloads on store changes, or (c) the flag-on desktop is incomplete until P47. The planning default is (c), which D21 takes. If it chooses (a) or (b), P41–P46's interim flag-path scope is revisited before P41 starts.
- **Done when:**
  - `WorkspaceHandle::{send, subscribe}`, the `Command`/`Event` enums and the actor loop exist; effects follow `sync.rs:1`–`:16`'s read → off-actor fetch → typed `Msg` apply; the actor thread makes no port call.
  - The read path exists: `send` accepts a `Query` as well as a `Command` and replies with the state the snapshot builder needs, or a read-only `WorkspaceView` handle is published per revision. The ADR records which, and phase 5's resources replace it.
  - P40 adds **Workspace** to `CONTEXT.md` (the "workspace" _Avoid_ line moves) and an ADR for the actor and the crate graph. The ADR notes how ADR 0015's off-main-thread command rule changes on the flag path: `send` returns a `Receiver`, and a command that blocks on it inside `run_blocking` must bound the wait.
  - Open-target commands route through the service with `v1-service` on; the flag-off path is unchanged.
  - Latency stays within 10% or 2 ms of the flag-off median, whichever is larger, over N runs on open target, mark reviewed and next file.
- **Planted breaks:** a port call on the actor thread trips the thread guard; removing the event send leaves the subscriber waiting; a flag-on command that still locks `App` is caught by a test that holds the `App` mutex and asserts the command returns within a bound; a command that waits unbounded on the reply fails the bounded-wait test.
- **Fallback split:** P40.1 protocol, actor loop and read path; P40.2 the first use case and the latency measurement.
- Size: L.

### P41 Annotation commands and the override deletion (review hardest)

- Branch: `epic/p41-annotation-commands`. Lanes: E-svc, F. Model: `opus`. Depends on: P40.
- **Goal:** reviewed set, questions, notes, comments and promote are Commands with explicit parameters; the client-supplied id survives as `client_id`.
- **Done when:** the three override fields (`mod.rs:862`, `:867`, `:881`), their take sites (`comments.rs:887`, `:892`–`:893`, `:1169`, `:1224`–`:1225`, `:1254`, `:1321`–`:1322`, `:1351`, `:1400`–`:1401`, `:1642`–`:1643`), the clear at `comments.rs:1169`, the five constructor literal blocks (`mod.rs:1616`–`:1620`, `:1760`–`:1764`, `:1898`–`:1902`, `:2054`–`:2058`, `:10054`–`:10058`) and the desktop setters (`commands.rs:2514`, `:2560`, `:9509`) are replaced by an explicit `client_id` parameter, as in the proposal's `AddQuestion { …, client_id }`; the optimistic id reuse (`optimisticLocal.ts:1298`) still lands the confirming snapshot on the same row; promote stays the one conversion into a `GithubComment`; the annotation desktop commands are routed on the flag path.
- **Planted breaks:** dropping the id parameter makes the optimistic-confirm test fail because the confirmed comment gets a fresh id; a command that reads the current selection fails the explicit-parameter test; a promote that writes the private annotation file fails the privacy test.
- **Fallback split:** P41.1 reviewed/questions/notes; P41.2 comments and promote.
- Size: L.

### P42 Sync and GitHub commands

- Branch: `epic/p42-sync-commands`. Lanes: E-svc, F. Model: `opus`. Depends on: P41.
- **Goal:** the `sync.rs` use cases and GitHub posting run as Commands against the `GitHub` port.
- **Done when:** snapshot → fetch → apply runs as one command; a fake-backed test drives comment sync and pending-review posting; the flag path updates from `Event`s without polling (the 30 s safety-net timer stays); error classification stays in `er-github` (ADR 0055); the sync and post desktop commands are routed.
- **Planted breaks:** a GitHub call made on the actor thread trips the thread guard; a sync that applies as a second writer fails the one-writer test; a flag-path update that polls the old revision path fails the event test.
- **Fallback split:** P42.1 sync; P42.2 pending-review post.
- Size: M.

### P43 Job and workflow commands

- Branch: `epic/p43-workflow-commands`. Lanes: W, E-svc, F. Model: `opus`. Depends on: P42 (lane order only; its only model edge is the shared lane), the P32a/P32b workflow engine and P33a (merged at the gate).
- **Goal:** `StartJob`, `CancelJob` and `JobProgress` are Commands over the post-P33 runner and workflow record; `arena_commands.rs` (20 commands) routes.
- **Done when:** each stage launch is a `JobSpec` submitted through the actor; `PanelRun`/workflow progress arrives as Events; the flag-off path keeps today's behaviour; a `CancelJob` sent through the actor reaches the runner; the Deep cancel/orphan test from P28b/P32b is reused unchanged.
- **Planted breaks:** a job spawned on the actor thread trips the no-IO test; a `CancelJob` that does not reach the runner fails the new assertion; a load that spawns fails the no-spawn test.
- **Fallback split:** P43.1 job commands; P43.2 workflow events and resume plumbing.
- Size: L.

### P44 Settings schema

- Branch: `epic/p44-settings-schema`. Lanes: E-svc, F. Model: `opus`. Depends on: P37 (merged at `738919af5`) and P40.
- **Goal:** the settings schema replaces the snapshot type at the engine root, and the flag-on config hub runs on it.
- **Done when:** the schema is generated from the config types and served through the service; labels, descriptions, options and sections come from `schemars` attributes or the hand-declared table (`config/settings.rs:60`), settled in a design issue; the snapshot's non-leaf parts are assigned: `risk_rules`/`risk_default` become a risk-rules resource, `risk_files` a per-tab query, and P44 deletes the `importance_*` DTOs and fields (`desktop_settings.rs:126`–`:128`) while P31 already deleted the config side they read; `DesktopSettingsSnapshot` moves out of `er-engine` into `er-desktop` for the flag-off path, and phase 5 deletes it with `snapshot.rs`; `config_commands.rs` (11 commands) routes on the flag path; `apply_config_field` (`config/desktop_settings.rs:231`) stays in the engine's non-`ui` config, reachable by both paths, and the ownership rule is: on the flag path, settings writes go through a service command; a field-for-field test covers every leaf against the `agents.toml`/`config.toml` split.
- **Planted breaks:** dropping one field from the schema fails the field-for-field test; a flag-on `config_commands.rs` call that writes config without the service command fails the routing test (the held-mutex form from P40), while the flag-off path keeps calling the function directly; a risk-rules query that reads the config file directly fails the resource test.
- **Fallback split:** P44.1 schema and engine; P44.2 front ends.
- Size: L.

### P45 Tab persistence and open-target caches

- Branch: `epic/p45-service-registries`. Lanes: E-svc, F. Model: `opus`. Depends on: P40, P42 (the caches call the `GitHub` port plumbing) and P36 (merged).
- **Goal:** the Workspace owns the tab list, tab persistence and the open-target caches; shared code both paths call sits in the engine's non-`ui` half; the app-scoped desktop state stays until phase 6.
- **Done when:** the tab record and its persistence (`TabsFile:51`, `save_tabs:70`, `save_app_tabs:81`, `persist_app_tabs:107`, `load_tabs:115`, persistence `:51`–`:121`), `pr_open_cache.rs` (with `PrOpenFreshness` lifted out of `commands.rs:230`; its import sits at `pr_open_cache.rs:116`) and `remote_pr_open_cache.rs` move into the engine's always-compiled half beside `sync.rs`; the code that builds a `TabState` from a descriptor (`tabs.rs:149`, `:331`, `:363`, `:373`) stays in `er-desktop` for the flag-off path, and the actor builds its own tab state on the flag path; their tests move with them; `er-mcp` builds against the moved modules; registry commands for tabs and open targets route on the flag path; the P36 per-front-end registry test covers the new home; the in-memory `remote_pr_open_cache` keeps its in-memory design.
- **Planted breaks:** the per-front-end registry test (P36's) fails if a kind is missing; a second writer to the tabs record fails the one-writer test; a cache that bypasses the port fails the fake-port test.
- Size: M.

### P46 Single-owner view state into the front ends

- Branch: `epic/p46-view-state`. Lanes: E-svc, F. Model: `opus`. Depends on: P40, P41.
- **Goal:** the view state with one front-end owner leaves `TabState` and `App`.
- **Done when:** `browser_url`, `browser_layout`, `browser_split_ratio`, `browser_annotate_mode`, `browser_show_tooltips` (`mod.rs:1098`–`:1106`) and `panels_visible` (`:5909`) move to the front ends; `file_tree_width` and `panel_width` (`:736`/`:739`) stay for 5b with the TUI layout group; the PR that first moves view state supersedes ADR 0006; the field list lives in the issue body, not in this addendum. Selection, cursor, scroll, overlay, input mode and `navigation.rs` stay for 5b (D20).
- **Planted breaks:** a view field read by the service fails the boundary test; a front-end file that writes domain state fails the ownership test; a desktop layout field left on `TabState` fails the field-list test.
- Size: M.

### P47.1 Snapshot builder over the read API

- Branch: `epic/p47a-snapshot-builder`. Lanes: F, E-svc. Model: `opus`. Depends on: P38–P46, especially P40's read API, P44 and P46.
- **Goal:** the flag-on backend builds the same snapshot DTOs from service state, without reading `TabState`.
- **Done when:** `snapshot.rs`'s builder (`build_snapshot_with_delta:3079`) reads through P40's `Query` reply or `WorkspaceView`; the DTO view fields come from a named desktop-side view struct (`DesktopViewState`: `selected_file`, `current_hunk`, `current_line`, `diff_scroll`, `h_scroll`, the desktop's scroll positions), not from `TabState`; the flag path reads no `TabState` view field; service `Event`s bump the existing revision (ADR 0011); pure navigation (next and previous file, hunk and line; jump to unreviewed) is extracted from `navigation.rs` (`next_file:4`, `prev_file:94`, `next_hunk:176`, `prev_hunk:188`, `next_line:209`, `prev_line:257`) and `App::next_unreviewed_file` (`mod.rs:9551`) into the engine's non-`ui` half, `App` keeps calling it on the flag-off path, and the desktop calls it over `DesktopViewState` with diff data from the read path; D20 keeps `navigation.rs`'s `App` methods and the `TabState` fields in 5b, and the extracted functions are pure and shared; latency stays within 10% or 2 ms of the flag-off median, whichever is larger, over N runs on open target, mark reviewed and next file.
- **Planted breaks:** a builder that reads a `TabState` view field fails the boundary test; a revision not bumped on an `Event` fails the refresh test; a flag-on `next_file` that locks `App` fails the held-mutex test; a latency regression past the budget fails the measurement.
- **Fallback split:** P47.1a the snapshot builder and `DesktopViewState`; P47.1b navigation and the latency measurement.
- Size: L.

### P47.2 Routing the remaining commands and the phase exit

- Branch: `epic/p47b-desktop-commands`. Lanes: F, E-svc, C. Model: `opus`. Depends on: P47.1.
- **Goal:** the flag-on desktop runs the remaining open, diff and file use cases through the Workspace; the flag-off path is unchanged; the phase's exit criteria hold.
- **Done when:** the open, diff and file commands route through `WorkspaceHandle` with no `App` lock on the flag path; view-only commands stay in the front end and are listed in the issue body; desktop-ui changes only where parameters changed; `er-core` has no IO, no `comment_*_override` remains, the schema has replaced the snapshot at the engine root, and the flag-off suite passes.
- **Planted breaks:** a command that still locks `App` on the flag path fails the routing test; a compile error planted behind `cfg(feature = "v1-service")` fails the CI `test-v1` job on a draft PR, as #470 did; a flag-off regression fails the old suite.
- Size: M.

**P48 (retire the legacy auto-triage flag, D22) shipped in phase 3 as #503
(`8460a7e22`).** The flag, its two commands, `should_queue_auto_triage` and the
settings toggle are gone; `retired_auto_triage_set` (`projects_pins.rs:209`) and
the D18-shape notice (`comments.rs:951`, call `:2935`) exist, and #503's own
tests cover it. P48 is dropped from phase 4. `auto_triage_max_diff_kb` is a
different live setting and stays.

## Lanes and schedule

| Lane | Hot files |
|---|---|
| **E-core** | `crates/er-core/**`, `ai/**`, `config/{risk_rules,path_rules}.rs` |
| **E-svc** | `crates/er-engine/src/app/state/**` except `review_flow*` and `app/state/arena.rs`, `sync.rs`, the rest of `config/**`, `crates/er-engine/src/storage.rs`, `ports.rs`, `store_registry.rs`, `lib.rs`, plus `crates/er-service/**` |
| **W** workflows | `app/state/review_flow*`, `app/state/arena.rs`, `arena/**` |
| **F** front ends | `er-desktop/src/{commands,snapshot,main,tabs,inbox,projects,pr_cache,gh_status_cache,pr_open_cache,remote_pr_open_cache,auto_triage,arena_commands,config_commands}.rs`, `desktop-ui/src/lib/**`, `er-tui/src/input/**`, `er-tui/src/main.rs`, `crates/er-mcp/**` |
| **C** crates, config, gate, CI | root `Cargo.toml`/`Cargo.lock`, `justfile`, `.github/workflows/ci.yml`, `scripts/tauri-dev.sh`, `crates/er-lint-gate/**`, `crates/er-store/src/**`, `crates/er-git/**` and `crates/er-github/**` (the P39 re-exports), new crate manifests and `clippy.toml` |

Schedule: the chain, with the three parallel pairs the review found. Each pair
is adopted; lanes are taken as written, and the coarse-lane pairs use
conflict-by-addition files (see Notes).

- **Chain:** P39.1 → { P39.2 → P39.3 ∥ P40 → P41 } → P42 → P43 →
  { P44 ∥ P45 } → P46 → P47.1 → P47.2; the braces are the adopted pairs, and
  each merge point waits for both sides. P48 shipped in phase 3 (see above).

- **(a) P38 in the phase-3 window.** P38 needs no phase-3 code; the coordinator
  exempts it from "no phase-4 entry starts until phase 3 closes", and it runs
  whenever no phase-3 entry holds C or F: P30a, P30b.1, P30b.2, P31, P36 and
  P37.
- **(b) P39.2/P39.3 beside P40/P41.** With E split into E-core and E-svc,
  P39.2 holds E-core, W and C, and P39.3 holds E-core and C; P40/P41 hold
  E-svc and F. Three conditions make it safe, all written into the entries:
  P38 declares `er-service`'s full dependency set and P40 holds E-svc only;
  P39.2 leaves one-line `pub use` shims in `config/risk_rules.rs`,
  `config/path_rules.rs` and `storage.rs`, so `config/mod.rs` is untouched and
  `storage.rs` sits in E-svc; that one-line shim is the pair's only share, and
  it resolves on rebase; P40 and P41 do not edit `crates/er-core/**` or `ai/**`
  while the pair runs, and any such need waits for P39.3.
- **(c) P44 beside P45 after P42.** Their files are disjoint (`config/**` and
  `config_commands.rs` against `tabs.rs`, `pr_open_cache.rs`,
  `remote_pr_open_cache.rs`); both hold E-svc and F, so this pair runs as a
  planned exception. The conflict-by-addition files are the `Command`/`Event`
  variants, the `main.rs` command registration, `desktop-ui/src/lib/types.ts`
  and the engine's `lib.rs`; all edits are additive or separate removals, so
  they resolve on rebase.

D19's split (moving the app-scoped desktop half to phase 6) shortens P45; a
finer F lane would make pair (c) lane-clean. P48 shipped in phase 3 as #503.

## Risks

1. **Dual ownership on the flag path.** Until P47, the service and `App` can
   each hold a loaded copy of one tab; a write through one is invisible to the
   other in memory, and store change events cover artifacts only. The design
   issue before P40 settles the model.
2. **The schedule is a chain with three pairs.** Seven L entries (P39.1,
   P39.2, P40, P41, P43, P44, P47.1), not eight; the adopted parallel pairs
   shorten the chain, and D19's split plus a finer F lane are the remaining
   levers.
3. **v0.6.x fixes reach only the flag-off path.** D2 merges `main` at each
   gate; a fix to `commands.rs`, `snapshot.rs` or `comments.rs` lands in the
   old path. Add "port flag-off fixes merged from `main`" to the gate steps for
   phases 4 and 5.
4. **TUI regressions with no library target.** P41 and P46 change `App` methods
   the TUI calls from `er-tui/src/input/**`; the TUI has no lib target, so its
   key paths are tested only through `App`. ADR 0006 records this failure mode.
5. **Reply-wait deadlock.** A Tauri command that blocks on `send`'s receiver
   inside `run_blocking` while the actor waits on something the same path holds
   deadlocks. P27's undrained-channel deadlock is the precedent; P40 carries a
   bounded-wait test.
6. **`TabState` view state that two front ends read.** Selection, cursor,
   scroll, overlay and input mode stay until 5b; the phase-4 flag path must not
   read them, or the service learns front-end state it cannot own.
7. **The trait move is layout-adjacent.** Moving `Artifact` and `SlotName`
   changes where the store's typed keys come from; review it as an API change
   and pin the on-disk format with the store key-path test.
8. **The actor can become the next god object.** Explicit command parameters,
   no view state and the actor-thread guard are the three rules that keep it
   from absorbing the front ends.
9. **P43 rests on a workflow engine that now exists** (P32a/P32b/P33a landed).
   Its anchors are re-checked above; a changed stage list changes the command
   set, not the actor design.
10. **`er-core` must build headless.** `er-mcp` builds the engine without `ui`;
    every entry that touches the core runs `build-mcp`.
11. **Machine load.** P38–P47 are mostly L; the three-lane ceiling and the
    machine-wide check lock bound the schedule.
12. **Feature unification hides a `ui` leak.** A workspace build compiles
    `er-engine` with `ui` whenever `er-tui` or `er-desktop` is in the graph, so
    an `er_engine::app::…` reference inside `er-service` passes `cargo test`,
    `test-v1` and the clippy gate, and fails only a standalone build.
    `build-service-headless` is the check, and P38's planted break proves it.
13. **New store kinds for view files.** If any per-front-end view file becomes a
    store artifact, it is a kind users keep (ADR 0056) and P36's per-front-end
    registry must admit it. Phase 4 does not add one (A15 defers the TUI file),
    so this is a 5b check.
14. **Actor thread count.** Multi-worktree tabs mean one actor thread per open
    repo; state the bound or idle-exit actors with no tabs.

## ADR and doc changes

- P40 adds the Workspace actor and crate-graph ADR, adds **Workspace** to
  `CONTEXT.md` and notes ADR 0015's off-main-thread rule on the flag path.
- The PR that first moves view state out of `TabState` supersedes ADR 0006;
  under D20, that PR is P46 and the ADR text says single-owner fields moved
  first.
- Phase 5 keeps the protocol ADRs (0011, 0012, 0016, 0017, 0018, 0033, 0034) as
  the proposal says; phase 4 cites 0017 (optimistic writes) in P41 and 0011
  (push revision) in P47.
- Phase 4 entries cite ADRs 0002 (synchronous engine), 0006, 0011, 0015, 0017,
  0021 (agent concurrency), 0055 (gh error classification), 0056 (store
  format) and 0057 (store lock scope).

## Notes

- P42's stop line: `github_post.rs` plus `sync.rs` may exceed it; the fallback
  split is above. (`github_sync.rs` does not exist; the always-compiled core
  is `sync.rs`.)
- P38's second clippy pass repeats the first pass's diagnostics for shared
  code; confirm the budget and prune paths handle duplicate locations before
  relying on them.
- Dependency edges among P38–P47 have no cycle. P45 → P42 is a model edge (the
  caches call the `GitHub` port); P43 → P42 is lane order only.
- Field lists belong in each issue body, which ADDENDUM-03 makes the spec; the
  claims section cites evidence, not the spec.
- Conflict-by-addition files for the adopted parallel pairs: `crates/er-service`'s
  `Command`/`Event` enums and the desktop's command registration in `main.rs`
  gain variants from two entries; `desktop-ui/src/lib/types.ts` changes DTO
  types in P44 and P45; and the engine's `lib.rs` loses the `:11`–`:14`
  re-exports (P44) while it gains the non-`ui` module (P45). The edits are
  additive or separate removals and resolve on rebase.

## Re-verification of the review's wrong claims

All eleven were re-checked at `60c9571ff` and again here at `afd78bced`; each
stands unless noted, with these results:

1. `Artifact::migrate` returns `StoreError` (io error, `HolderInfo`, `PathBuf`,
   `MAX_ARTIFACT_BYTES`), and `type Slot: SlotName` exists (`artifact.rs:18`).
   The "pure trait" wording is gone.
2. `git/diff.rs:1` imports `er_git::FileStatus`. P39.1 moves `FileStatus` down.
3. The projection's closure is now the accepted-findings input
   (`ai/accepted_findings.rs`, no IO, no arena imports); P34b deleted the
   `accepted_provenance` run read entirely. P39.3's P34b dependency is
   satisfied.
4. `storage_branches_match` calls `crate::storage::slug_branch`; `loader.rs`
   has 26 `std::fs` occurrences (was 62 at `60c9571ff`). Both the helper and
   `slug_branch` are in P39.2.
5. The arena rules import `model.rs`/`schema.rs` (identity does not), and
   `provenance.rs:4` needs the expert table. Both move in P39.2.
6. The crate has 199 commands, not 157 or 200; `arena_commands.rs` has 20 and
   `config_commands.rs` 11. Both are named in P43 and P44.
7. `App` has 37 field declarations now: 22 `pub`, 6 `pub(crate)`, 9 private.
   The count uses a brace-depth scan from `pub struct App {` to its matching
   `}` (`mod.rs:5839`–`:5974`), counting declarations at depth 1 that match
   visibility + name + `:`. P32a removed one `pub` field
   (`review_flow_leases`), which accounts for the drop from 38.
8. `save_session`/`restore_session` are called only from
   `er-tui/src/main.rs:315`, `:675`, `:701` and tests. The session split moved
   to the coordinator-settled list, with no phase-4 behaviour change.
   `session_store.rs` is at the crate root since #524.
9. No `gate_override` field or type exists; the phrase is removed.
10. `BranchView` has no serde derive and its engine users are test modules.
    P39 drops it; it moves when a core type first needs it.
11. The header links use `./PLAN.md` and `./ADDENDUM-*`, which resolve from the
    addendum's committed location.

Round-2 corrections on top: `App` is 37 by the method above;
`DesktopSettingsSnapshot` spans `desktop_settings.rs:110`–`:129` (unchanged by
P37's split); `er-desktop`'s `browser_*` hits include the
`browser_proxy`/`browser_webview` module paths, and `overlay` counts the AI
overlays too, so both are labelled word hits rather than field reads.

## Change list

- **A1** Crate graph defined at r2 as engine-depends-on-service; superseded by
  B1, which flips it to service-depends-on-engine with the use cases in the
  engine's always-compiled half.
- **A2** State ownership on the flag path: the three options are written down,
  a design issue settles it before P40, and O3 carried the owner question (now
  D21).
- **A3** Workspace redefined per repo/worktree, with remote-only tabs under a
  per-repo-slug workspace; `CONTEXT.md` entry and _Avoid_ move assigned to P40.
- **A4** `Artifact` reasons rewritten (orphan rule, `StoreError`, `SlotName`,
  migration error); the `Mode` mention is dropped in this re-check because
  P30b.2 deleted the type; `BranchView` dropped from P39; Q4 moved to the
  coordinator-settled list with its options.
- **A5** P39 scope rewritten from the real closure and split into P39.1–P39.3;
  dependency now P31, P33b, P34b; planted breaks replaced with the `as_str`
  key-path break and the `TypeId` test.
- **A6** `er::core_io` syn rule and a dependency allow-list added to P39, with
  planted breaks.
- **A7** P46 split per single-owner rule; field table and `navigation.rs`
  added; omitted `current_hunk`/`current_line` and App fields added; O2 carried
  the acceleration option and its split (now D20, which drops it).
- **A8** desktop-ui scope for phase 4 defined (same snapshot DTOs, events bump
  the revision; parameter changes only).
- **A9** Desktop routing spread across P41–P45; P47 keeps open, diff and file
  commands and the exit check; view-only commands listed in the issue; lane F
  gains `arena_commands.rs`, `config_commands.rs`, `remote_pr_open_cache.rs`.
- **A10** Decisions and owner questions separated; four owner decisions remain,
  each with options and a recommendation.
- **A11** Q1 reframed; P45 now moves tabs and open-target caches only and is M;
  inbox, projects, the app-scoped caches and auto-triage stay for phase 6.
- **A12** P38 proofs replaced: default-build `cfg!` test, boot-value mismatch
  test, and the feature-only warning plant.
- **A13** Q4/Q5/Q6 settled by the coordinator and listed there; test-v1 gated
  on `rust`/`ui` and added to `just ci`; the snapshot moves to `er-desktop`;
  P44 and Risks 7 are rewritten.
- **A14** Owner questions added: flag-on completeness (O3, now D21),
  auto-triage flag retirement (O4, now D22); the latency budget is a
  coordinator default.
- **A15** Q2 reframed on the TUI-only facts; `SessionState` becomes the TUI's
  file in 5b; no behaviour change; risk 5 rewritten.
- **A16** P41 done-when completed with setters, constructors, clear,
  `client_id`, the optimistic test and the "review hardest" mark.
- **A17** P44 scope fixed: risk resources, labels source design issue,
  `config_commands.rs`, `apply_config_field` privacy, and the risk-resource
  planted break.
- **A18** Lanes made disjoint (E drops `review_flow*`/`arena.rs`), new crates
  added to E, `er-mcp` added to F.
- **A19** Re-check list completed with P28b, P30b.1, P31's `Scope::Repo`
  anchors, P32a's field count, P33b's shim removal and P36's `session_store`
  move.
- **A20** Proofs fixed for P40 (mutex-bound routing test), P42 (stderr plant
  dropped; flag-path wording), P43 (reused orphan test plus `CancelJob`
  assertion), P45 (P36's test, in-memory cache, `er-mcp` build) and P47 (draft-PR
  compile plant).
- **A21** ADR and doc changes section added; P40 owns the actor/graph ADR,
  CONTEXT.md and the ADR 0015 note; ADR 0006 supersession assigned.
- **A22** ADDENDUM-04's L5 stop-line sentence copied.
- **A23** Header links fixed to `./PLAN.md` and `./ADDENDUM-*`; `App` corrected
  from "23" to 38 field declarations (23 `pub`, 6 `pub(crate)`, 9 private;
  the brace-depth method is in the claims); `sync.rs` wording fixed; the
  gate-override phrase replaced with the finding that no such field exists.

## Round-2 change list

- **B1** Crate graph replaced with model (d): use-case functions live in the
  engine's always-compiled half (`sync.rs` shape); `er-service` depends on
  `er-engine` with `default-features = false` and owns the actor; the use cases
  move up into the service when the old path is deleted in phases 5 and 5b. The
  rejection of the old option (a) is corrected with the `er-mcp` precedent.
- **B2** P39.1's dependency set named: `AiState` stays in the engine; the type
  halves of `arbiter`, `validation` and `identity` and `relocate.rs` move with
  the review types; `lenient_verdicts` follows the verdict; the non-test
  imports are written into the entry.
- **B3** P39.2 gains `config/path_rules.rs` and `glob` and holds lane W; the
  `glob::glob` planted break is added to `er::core_io`; `er-git/**` and
  `er-github/**` join lane C.
- **B4** The read path is defined in P40 (`Query` reply or `WorkspaceView`
  handle) and the desktop view struct is named in P47.1; P47 splits into
  P47.1 (snapshot builder) and P47.2 (routing and exit).
- **B5** `App` corrected to 38 field declarations (23 `pub`, 6 `pub(crate)`, 9
  private), with the brace-depth method recorded in the claims and
  re-verification.
- **B6** The latency budget is in P40's and P47.1's done-when, with a threshold
  and the three measured actions.
- **B7** The A2 design issue is in P40's dependencies; the revisit of P41–P46's
  interim scope and O3's default (now D21) is stated.
- **B8** O4's (now D22) reason is reworded: it removes a dead setting and
  lands as a small standalone PR, not a P45 shrink.
- **B9** P46 drops "the desktop's layout widths"; `file_tree_width` and
  `panel_width` stay for 5b, and P46 depends on P40/P41.
- **B10** The boot value comes from `er-desktop`, not the engine.
- **B11** The `importance_*` DTO deletion is assigned to P44, and P31's
  re-check bullet says so.
- **B12** Conflict-by-addition files are named for the adopted parallel pairs.
- **B13** The `overlay` and `browser_*` figures are labelled word hits.
- **B14** Risk 2 counts seven L entries, not eight, and reflects the three
  adopted parallel pairs.

## Round-3 change list

- **C1** Shared code goes in the engine's non-`ui` half; `er-service` holds
  only the actor, its commands and its read path. P45 moves the tab record,
  persistence and open-target caches beside `sync.rs`, with `PrOpenFreshness`
  lifted from `commands.rs` and the `TabState` builder staying in `er-desktop`;
  P44 keeps `apply_config_field` in the engine's non-`ui` config and restates
  the rule as "on the flag path, settings writes go through a service command";
  the crate-graph decision carries the one-line rule.
- **C2** Flag-path navigation is extracted into the engine's non-`ui` half in
  P47.1, with the P47.1a/b fallback split and the held-mutex `next_file` break;
  O2 (now D20) is unchanged.
- **C3** `just build-service-headless` (`cargo check -p er-service
  --all-targets`) joins P38, `test-v1` and CI, with the `er_engine::app::App`
  planted break and risk 12 for the feature-unification gap.
- **C4** Pair (b)'s three conditions are written into P38, P39.2 and P40/P41:
  P38 declares the service's full dependency set and P40 holds E-svc only;
  P39.2 leaves one-line re-export shims; P40/P41 do not edit E-core files
  during the pair.
- **C5** `crates/er-engine/src/storage.rs` joins E-svc, and E-core's `ai/**`
  row drops its qualifier, so `ai/loader.rs` and `ai/prompts.rs` are visibly
  owned.
- **C6** The `identity.rs` paths are qualified (`ai/identity.rs` in P39.1,
  `arena/identity.rs` in P39.2), and P39.3 moves only the `validation` and
  merge-function remainder.
- **C7** Pair (a)'s phase-3 holder list adds P30b.1 and P30b.2.
- **C8** Pair (c)'s conflict-by-addition list adds
  `desktop-ui/src/lib/types.ts` and the engine's `lib.rs`.
- **C9** "The flag path reads no `TabState` view field" moves from P46 to
  P47.1.
