# Addendum 06: phase 5, desktop protocol

Drafted for the phase-5 gate (#398), following [PLAN.md "Re-planning at a
phase gate"](./PLAN.md#re-planning-at-a-phase-gate). Where this file disagrees
with PLAN.md or an issue body, this file wins once committed;
[ADDENDUM-01](./ADDENDUM-01-decisions.md),
[ADDENDUM-02](./ADDENDUM-02-issue-decisions.md) (D11–D22) and
[ADDENDUM-05](./ADDENDUM-05-phase-4.md) still bind.

Every claim and citation below was checked against `release/v1.0.0` at
`949eb993d` (2026-11-17), the merge of #597. Two phase-4 PRs were still open at
that ref: #578 (P47.2b.2, head `88c21f507` on `epic/p47g-tab-keys`) and
P47.2c.2 (local head `d5e060baa` on `epic/p47i-parity-exit`, no PR yet). Their
sites are cited at those heads and marked **re-pin**: once both merge, every
line number in this file and in the issue step plans is re-found by symbol at
the merge head before the first build starts. Symbols are the anchor; line
numbers are a convenience.

## Claims re-checked against the tree

Method: `git grep` and `git show` at `949eb993d`; command signatures were
parsed from the line after each `#[tauri::command]` attribute to the opening
brace.

- **Command surface.** er-desktop has 205 `#[tauri::command]` functions with
  200 distinct names: `new_tab`, `close_tab`, `select_tab`, `reorder_tabs` and
  `submit_github_pr_decision` each have a flag-on and a flag-off body under
  `cfg`. All 200 names are registered in the one `generate_handler!` list
  (`main.rs:1988`–`:2189`). By file: `commands.rs` 159, `arena_commands.rs` 20,
  `config_commands.rs` 11, `browser_webview.rs` 9, `risk_commands.rs` 2, and
  one each in `commands/line_commits.rs`, `gh_pr_actions.rs`, `github_image.rs`
  and `main.rs` (`v1_service_enabled` `:890`). So 46 commands sit outside
  `commands.rs`, and the exit rule covers all of them.
- **Return types.** 129 commands return a type named `*Snapshot`
  (`AppSnapshot` 127, `FileSnapshot` 1, `AgentLogSnapshot` 1); three more
  return `ArenaRunSnapshotWire`, whose name contains `Snapshot` without ending
  in it. 201 return `Result<_, String>`. Four return neither:
  `dev_log_filter` (`arena_commands.rs:700`), `list_ai_experts`
  (`commands.rs:4496`), `list_ai_reviewers` (`:4501`) and
  `v1_service_enabled`.
- **Snapshot types.** er-desktop declares 42 `pub struct *Snapshot` types:
  40 in `snapshot.rs`, `SettingsSnapshot` (`settings.rs:12`) and
  `TriageReviewUpdateSnapshot` (`triage_update.rs:13`). `AppSnapshot`
  (`snapshot.rs:458`) has 61 top-level fields. `snapshot.rs` is 8,341 lines,
  `commands.rs` 16,411, `main.rs` 2,747.
- **The old protocol's moving parts.** `poll` (`commands.rs:11864`,
  `poll_impl` `:11869`), `compute_chrome_revision` (`:12060`) and
  `compute_content_revision` (`:12127`), `snap_from_command` (`:359`) and
  `snap_from_confirmed` (`:381`), `SentFilesState` (`snapshot.rs:66`) with
  `SENT_FILES_VIEW_CAP` (`:59`) and `record_sent_file` (`:189`),
  `SNAPSHOT_DIFF_LINE_BUDGET` (`:285`), and the revision emitter
  `run_revision_watcher` (`main.rs:855`), which debounces 40 ms and emits
  `er://revision`. The per-file content key is `file_lines_key`
  (`snapshot.rs:105`, ADR 0013); the delta key `file_delta_key` (`:137`) also
  hashes the file's threads (ADR 0012).
- **desktop-ui.** `types.ts` is 1,179 hand-written lines with 83 exported
  types, plus `types/arena.ts` (233). `app.svelte.ts` (1,275 lines) holds the
  one `snapshot` store, the 30 s safety timer (`pollIntervalMs` `:285`,
  `startPolling` `:575`), the `er://revision` listener (`:634`), `cmd`
  (`:1025`), `cmdOptimistic` (`:864`) and `cmdReviewed` (`:972`).
  `App.svelte:293` has a second `er://revision` listener for the arena page.
  `optimisticLocal.ts` (1,302 lines) lists 28 `OPTIMISTIC_COMMANDS` (`:24`) and
  22 `OptimisticOp` variants (`:76`), four of them global
  (`inbox-read`, `inbox-clear`, `saved-pr`, `dismissed-pr`); the reviewed
  checkmark is a separate path through `cmdReviewed`. The proposal's "36
  operation kinds" is stale. `snapshotDelta.ts` (54), `snapshotChrome.ts`
  (142) and `tabSnapshotCache.ts` (153, `TAB_SNAPSHOT_CACHE_CAP = 8` `:4`)
  carry the merge and cache rules. 106 non-test files read `app.snapshot` or
  name `AppSnapshot`.
- **What the service already publishes** (ADR 0067, 0069, 0070, 0071).
  `WorkspaceView` (`er-service/src/workspace.rs:45`) carries the targets, the
  live arena runs, the review flows and `tabs: BTreeMap<TabKey,
  Arc<TabContent>>`. `TabContent` (`content.rs:28`) has four sections, each
  stamped with the revision that last replaced it: `DiffSection` (`:231`),
  `ReviewedSection` (`:258`), `AiSection` (`:267`) and `PrSection` (`:278`),
  plus `mode_revision`. `TabsView` (`app_registry.rs:49`) has its own
  revision, and `SettingsView` (`settings.rs:131`) carries the config hub rows
  and one `RiskRulesView` per rules key. `Event` (`protocol.rs:682`) has
  `ViewPublished`, `TabsChanged` and the job events. `Command`
  (`protocol.rs:27`) gates `Ping`, `PortProbe` and `Stall` to `cfg(test)`, but
  its variants carry `TabContext` and engine types, so it cannot be the wire
  enum as it stands.
- **The flag.** 283 `cfg(feature = "v1-service")` lines in
  `er-desktop/src`, 187 of them in `commands.rs`. `just test-v1` runs
  `er-desktop --lib` and `er-tui` with the feature; CI's `Test (v1-service)`
  job (`ci.yml:83`) runs the same two. desktop-ui checks the build value in
  `v1ServiceFlag.ts`.
- **The engine split.** Only `app` and `export` are `ui`-gated in
  `er-engine/src/lib.rs`; `arena` and `navigation` are always compiled.
  er-desktop depends on er-engine with default features and names
  `er_engine::app::` 150 times (98 in `commands.rs`). Its `export.rs` calls
  the `ui`-gated `er_engine::export`.
- **No codegen exists.** No crate depends on `ts-rs` or `specta`.
- **The sidebar's PR fingerprint ignores `head_oid`.** `build_projects`
  (`snapshot.rs:4552`) memoizes the project rows behind `ProjectsCacheKey`
  (`:4581`); `build_projects_cache_key` (`:4592`) hashes each remote's PR count
  and last fetch time. The 30 s head probe (`main.rs:1316`–`:1370`) calls
  `patch_pr_head_oid` (`pr_cache.rs:114`, call `main.rs:1362`), which changes
  one row's `head_oid` in place and touches neither, so the sidebar keeps the
  old oid until the next full sweep. The coordinator's "about line 4484" was a
  `main` line number; at this ref it is `:4592`. The probe also picks its
  target from the App's active tab (`pr_head_probe_target`).
- **Sidebar sources are desktop-side** (D19): projects (`projects.rs`), the
  branch metadata cache (`ProjectMeta` `snapshot.rs:2375`,
  `refresh_meta_cache` `:2386`), the PR list cache (`pr_cache.rs`), GitHub
  status (`gh_status_cache.rs`), inbox (`InboxState` `inbox.rs:167`, read by
  `snapshot_inbox` `snapshot.rs:943`) and auto-triage (`auto_triage.rs`).
- **The read seam exists.** `SnapshotRead` (`snapshot.rs:3099`), `AppRead`
  (`:3142`), `build_snapshot_from` (`:3274`) and `ViewRead`
  (`snapshot_view.rs:27`) came with P47.1a; `DesktopViewState`
  (`view_state.rs:20`) holds panels, browser state and `TabNavState` (`:73`)
  per `TabKey`.
- **Latency harnesses.** Two ignored harnesses exist:
  `the_actor_hop_stays_within_the_latency_budget`
  (`er-service/src/tests.rs:209`) and
  `the_content_routes_stay_within_the_latency_budget`
  (`content_tests.rs:464`).
- **`main` is three commits ahead of release**, all CI (`f107a0944` merges
  #575: staging PRs and the CRAP report). The D2 merge touches only
  `.github/workflows/ci.yml`.

## Phase-4 changes as landed

P38 through P47.1 are merged, and P47.2's layers 2a.1–2a.3 (#570, #572–#574,
#576), 2b.1 (#577) and 2c.1 (#595, #596, #597) are merged at `949eb993d`. What
phase 5 builds on:

- **The service owns tab state and routes** (owner, 2026-11-16, P47.2c slim).
  The open, diff, file, reviewed and annotation routes run through the
  Workspace on the flag path; the flag-on snapshot still reads the App, through
  `AppRead`. Phase 5 replaces that rendering.
- **Tab content is published** as `Arc` sections with their own revisions (ADR
  0071), including the PR section, preview stamps, every diff mode recorded,
  and `ReloadSidecars` from the poll tick (#596).
- **One actor key and one tab key.** `workspace_key` replaced `service_key`
  (#577); #578 makes the registry issue every key and the App follow it by key
  (**re-pin**).
- **App-scoped records have one service owner** (ADR 0070): `AppRegistry` for
  tabs and the open caches, the settings actor for config (ADR 0069). The
  sidebar sources stay in the desktop under D19.

### Phase-4 leftovers and where each goes

These come from #578's follow-ups and review, #596 and P47.2c.2. All are
**re-pin**: re-check each against the merge heads.

- Followed and restored stubs never get their first diff with the flag on →
  **P56.1** (fetching `diff(tab)` for a key with no content sends `LoadTab`).
- The background tab warmer (`main.rs:1724`) is off with the flag on →
  **P56.1** (warm unloaded registry tabs through `LoadTab` at idle).
- `DropTab` publishes a revision for a key that never loaded → **P54** (a
  drop of a never-loaded key moves no resource version).
- An all-broken restore keeps one broken tab → **P54**.
- A close that lands before the actor starts sends no `DropTab`
  (`send_if_started`, `service.rs:184` at `88c21f507`) → **P54** (a
  `LoadTab` for a key the registry no longer lists is refused).
- `WorkspaceState::closed` grows for the life of the actor
  (`workspace.rs:153` at `88c21f507`) → **P54** (bounded, with a test).
- `same_pr` requires the same tab kind (`content_routes.rs:122` at
  `88c21f507`) → **P54**, as a proposal below.
- `launch_root` (`main.rs:1059`) and the warmer scope (`:1008`) read the
  startup tab on the flag path → **P54** and **P56.1**; `auto_register`
  (`projects.rs:523`) → **P55.1**.
- `toggle_reviewed` and `mark_reviewed` (`write_reviewed`,
  `content_routes.rs:372`) have a follower-lag gap → **P57**: the command
  carries the `TabKey` the page rendered. P62.2 deletes the follower, which
  closes the class.
- The sidecar reload watches `er_dir` only (#596; `readers`,
  `sidecar_reload.rs:115`) → **P58.1**.
- Cmd-T on a remote PR tab opens a rootless Working tab (predates phase 4) →
  **P54**.
- **"Left on the App with the flag on"** (provisional; the coordinator
  confirms the final list from P47.2c.2 before approval):
  - rendering: `get_snapshot` and `poll` build every `AppSnapshot` section
    through `AppRead` → P53–P60 port each section; P62.2 deletes both;
  - History, Conflicts and Hidden load on the App (`commands.rs:1333`), and
    History file clicks reach `history_select_file` (P47.2c.2) → **P56.3**;
  - compaction expansion (`toggle_compacted`), mtime sort, file and image
    previews and `list_diff_paths` → **P56.2**;
  - GitHub senders keyed off the App tab: `active_tab_command`
    (`commands.rs:6836`), `send_for_active_tab` (`:6845`) and the comment
    tick, plus `abort_wrong_view` (`:343`) and the document preview check →
    **P59**;
  - arena `resolve` (`arena_service.rs:55`) and flow `key`
    (`flow_service.rs:39`) → **P60.1**;
  - the PR head probe's target (`pr_head_probe_target`) → **P55.2**;
  - the App follower (`tabs::follow_registry`, #578) → deleted in **P62.2**.

## Gate steps (coordinator, before P50 starts)

1. #578 and P47.2c.2 merged and #538 closed with the exit checklist and the
   "left on the App" list in its body; STATUS and BUILD_LOG updated.
2. **D2:** merge `main` into `release/v1.0.0` (three CI commits at
   `f107a0944`) and run `just ci` through the lock. This is a gate step, not an
   entry.
3. Re-pin every **re-pin** citation here and in the step plans at the merge
   head; record what moved in this file's Notes.
4. The coordinator confirms the proposals below and the owner approves the
   plan; then the issues open as sub-issues of #371.

## Decisions settled by the coordinator

Each is a planning proposal the coordinator confirms or changes at the gate.

- **A1 Crate shape.** `er-protocol` is a leaf crate of wire types: serde,
  `ts-rs`, and nothing from the workspace. A dependency allow-list in the
  existing `er::core_io` rule holds it there. Deriving `TS` on er-core or
  engine types was rejected: it would put `ts-rs` on er-core's allow-list
  (P39) and tie the wire to internal types. The builders that map service
  views to wire types live in `er-service` (workspace resources) and in
  `er-desktop` (sidebar, chrome, window), so the TUI and MCP can reuse the
  service builders in 5b and 6.
- **A2 The hand-typed surface.** Three commands and one event, typed by hand
  in one TypeScript client file: `er_send(command) -> Result<Ack, ErError>`,
  `er_fetch(keys) -> Result<Vec<Versioned<Resource>>, ErError>` (batched),
  and `er_versions(keys) -> Result<Vec<ResourceVersion>, ErError>`, which
  answers the listener-attach race the 30 s safety net covers today. The event
  `er://resources` carries coalesced `ResourceChanged { key, version }` lists;
  Tauri's event channel is the subscribe. `Ack` carries the versions the
  command moved and, for a create, the id. `ErError { kind, message }` maps
  `WorkspaceError` (including `Github(GhError)`, ADR 0055) to a kind the
  frontend branches on. The wire command enum lives in er-protocol and names
  tabs by `TabKey`; the desktop adapter resolves the `TabContext` from the
  published content, as the phase-4 content routes do.
- **A3 Resources, owners and versions.** A resource's version is the
  revision its one writer stamped when it last replaced it, so no hash
  fingerprint decides freshness.
  - Workspace (er-service builders, versions from ADR 0071's section
    revisions): `diff(tab)` (file rows with a content hash, stats, mode,
    branch and base, staleness stamps), `hunks(hash)`, `reviewed(tab)`,
    `annotations(tab)` (the AI section: findings projection, questions,
    notes, GitHub comments, threads, tour, checklist, triage, diagrams, the
    other bucket), `pr(tab)`.
  - App registry: `tabs` (keys, descriptors, active key, labels; a tab's
    staleness is derived in the frontend from `prs(remote)`).
  - Settings actor: `settings` and `risk(rules key)`.
  - Desktop app owner (D19, owner answer): `projects`, `branches(project)`,
    `prs(remote)`, `inbox`, and `pr_status(remote, number)` from the GitHub
    status cache.
  - Desktop adapter: `jobs` (one app-wide resource built from each live
    actor's jobs, arena and flow sections plus the auto-triage queue; its
    version is a desktop counter the relay bumps when any part moves),
    `session` (watch status, app version, active AI labels, config load
    error), `view(tab)` (browser pane, ADR 0068) and `panels`, and
    `ui_annotations(tab)`.
  - Notices become an `er://notice` event, since a notice is a moment, not
    state.
- **A4 Content addressing.** `hunks(hash)` is keyed by the hash of one file's
  own hunk lines, `file_lines_key`'s rule (ADR 0013), computed by the service
  builder. Threads leave the hunk payload: they come from `annotations(tab)`
  and DiffView joins the two by path and line, so a comment no longer
  re-sends a file. A hunks resource is immutable, so it has no version; the
  frontend keeps an LRU cache bounded by bytes. A fetch for a file the
  section has not parsed sends `ParseFiles` and waits within ADR 0067's bound,
  so there is still one parse path. This deletes `SentFilesState`,
  `delta_key`, `hunks_omitted`, the lazy-stub downgrade and both cap-8
  constants.
- **A5 Porting a store with the flag on and off.** Each entry adds one
  adapter store per resource with one read API, fed from an `app.snapshot`
  slice with the flag off and from `er_fetch` with the flag on, and one write
  API that calls the old command or `er_send`. Components move to the adapter
  once. Each issue body lists the `AppSnapshot` fields and the commands the
  entry stops using, with `file:line`; the 61 fields and 205 commands are each
  assigned to exactly one entry.
- **A6 The exit rule lands early, in D7's shape.** P51 adds
  `er::command_shape` at error tier: a `#[tauri::command]` function must
  return `Result<T, ErError>` or a type with no `Result` and no `String`
  error, and no path segment of its return type may end in `Snapshot`. Its
  allow-list freezes today's 200 names and may only shrink; P61 shrinks it and
  P62.2 deletes it. Planted breaks cover the two direct shapes and a type alias
  (`type R = Result<X, String>`), which a literal match would miss.
- **A7 The protocol ADR.** P52.2 adds ADR 0072 (check the number at commit
  time) for the protocol: resources with versions, push invalidation per key,
  content-addressed hunks, `ErError`, the three commands, `ts-rs`, and
  ADR 0017's `client_id` rule carried over unchanged. The seven
  "Superseded by" marks on ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034
  land in P62.2, because the flag-off path runs under them until then.
- **A8 The optimistic layer.** Each of the 22 op kinds and the reviewed path
  is classified in the issue that ports it:
  - **pending until ack** (ADR 0017's `client_id`): thread and comment
    creation (`add-thread`, `reply`, `promote`, `promote-finding`, the `new`
    finding reply) and the reviewed checkmark on the one clicked path;
  - **rule moved to the server**: the saved-PR cap (`MAX_SAVED_PRS`
    `optimisticLocal.ts:74`), inbox unread counts, the dismissed-PR filter,
    `reviewed_count` arithmetic and thread ordering are computed by the
    resource builder;
  - **deleted**: every other paint, once its resource's push arrives within
    the latency budget (A9).
- **A9 Latency budget.** Old: the flag-off command plus
  `build_snapshot_with_delta` plus serialisation of the returned
  `AppSnapshot`. New: `er_send` to `Ack`, plus the changed resources'
  `er_fetch` and their serialisation. Measured in the ignored harnesses above,
  extended to er-desktop, over N runs on a 5,000-line diff, for open target,
  tab switch, mark reviewed, next file and add question. Budget: within 10% or
  2 ms of the old median, whichever is larger (ADDENDUM-05, B6). An action
  whose paint was optimistic is also timed click-to-paint in the desktop with
  `profileLog` and the numbers recorded in the PR; that row is reported, not
  asserted.
- **A10 Selection and navigation.** The frontend owns selection, cursor and
  scroll (the proposal's §3.5); `next_file` and its siblings become desktop-ui
  functions over `diff(tab)` and `reviewed(tab)`, so a keypress makes no IPC
  call. A JSON fixture of navigation cases runs against both the TypeScript
  functions and P47.1b's Rust functions, which stay for the TUI (D20), so the
  two cannot drift. `DesktopViewState` keeps browser state and panels, which
  window code reads. The alternative, a `view(tab)` command answered on the
  desktop, keeps one implementation but adds a round trip per keypress.
- **A11 Use cases stay in the engine.** ADDENDUM-05's B1 moves the use cases
  into er-service "when the old path is deleted in phases 5 and 5b". The TUI's
  `App` still calls them until 5b, and er-service depending on er-engine means
  they cannot move up while an `App` caller remains. They move in 5b.
- **A12 Flag deletion is desktop-only.** P62.2 deletes `v1-service` from
  er-desktop and keeps it on er-tui, with `cargo test -p er-tui --features
  v1-service` in `test-v1` and CI, and the lint gate's second pass for er-tui.
- **A13 The sidebar owner.** One desktop module, the app owner, holds the
  four sidebar resources and `pr_status`; each has one writer that bumps its
  version on write, and the owner sits behind a narrow trait so phase 6 moves
  it into the service without touching desktop-ui.
- **A14 Window-mechanics commands stay direct.** The browser webview,
  terminal, OS launch, export, reveal, line-commit, image-proxy, window-drag
  and dev-log commands touch the webview, a PTY or the OS shell and have no
  resource. They stay `#[tauri::command]` functions, typed by the rule
  (`Result<T, ErError>` with `T` from er-protocol); P61 converts them.
- **A15 `same_pr`.** An open of a PR that is already open selects the open tab
  whatever its kind (local PR or remote PR), when both name the same
  `workspace_key` and PR number. P54 implements it with a test.
- **A16 Generated files and lint.** The generated directory
  (`desktop-ui/src/lib/protocol/generated/`) is excluded from ESLint with a
  reason, because a hand fix would be overwritten; `check-ui` still
  type-checks it. On a merge conflict it is regenerated, never hand-merged,
  as with the clippy budget file.

## Owner decisions (decided)

Quoted from the coordinator's record (2026-11-15 14:10), re-confirmed by the
owner on 2026-11-17:

- "D9: ts-rs (types from the protocol structs; the ~3 commands typed by
  hand)."
- "Sidebar owner (phase 5): a desktop-side app owner publishes the sidebar
  resources through er-protocol, one writer per resource, bump on write. D19
  stands; it moves into the service in phase 6."
- "Sidebar granularity: per data source: `projects`, `branches(project)`,
  `prs(remote)` (rows plus server-built my-PRs/to-review/merged id lists) and
  `inbox`. `is_active`/`is_current` are derived in the frontend from `tabs`;
  triage spinners read `jobs`."
- "P47.2c SLIM (owner, 2026-11-16): the flag-on snapshot rendering moves to
  phase 5. Phase-4 exit = the service owns all tab state and routes."
- On 2026-11-17 the owner re-confirmed the D9 and sidebar answers and asked
  for an addendum, issues with step plans, and owner approval at the gate
  before builds start.

## PR entries

Shared acceptance: `fmt-check` and `clippy` always; `test`, `test-desktop`,
`test-all` (er-protocol and er-service are outside `just test`), `test-v1` and
`build-service-headless` as the touched crates require; `build-mcp` when engine
features or dependencies change; `docs-check` for docs and ADRs; and for every
entry that touches desktop-ui, `check-ui`, `lint-ui` and `test-ui`. Lint only
tightens (ADR 0038). Each entry from P52.1 on runs `protocol-check` (P50).
Until P62, every store entry leaves the desktop working with the flag on and
with it off; `test-ui` runs each adapter store's tests in both modes. The stop
line is 1,500 added-plus-changed lines after rename detection; pure deletions
are measured by what remains and do not count (ADDENDUM-04's L5).

### P50 `er-protocol`, ts-rs codegen and the drift check

- Branch: `epic/p50-er-protocol`. Lanes: C, P. Model: `opus`. Depends on: the gate steps above.
- **Goal:** the wire crate and generated TypeScript, before any resource uses them.
- **Done when:** `crates/er-protocol` exists as a leaf crate with `ResourceKey`, `ResourceVersion`, `Versioned<T>`, `ResourceChanged`, `Ack`, `ErError { kind, message }` and an empty wire command enum, each deriving serde and `TS`; `just protocol-gen` writes `desktop-ui/src/lib/protocol/generated/`; `just protocol-check` exports into a temporary directory and fails on any difference with the committed files, and CI runs it on the `rust` and `ui` change outputs; `protocol-check` joins `just ci`; the `er::core_io` dependency allow-list covers er-protocol (serde, ts-rs); er-protocol joins `just test`.
- **Planted breaks:** a hand edit of one generated file fails `protocol-check`; a new field on a protocol struct without regenerating fails it; adding `er-engine` to er-protocol's dependencies fails the allow-list; removing the CI step leaves the hand edit green on a draft PR.
- Size: M.

### P51 `er::command_shape` with a shrinking allow-list

- Branch: `epic/p51-command-shape-rule`. Lanes: C. Model: `opus`. Depends on: P50 (it names `ErError`).
- **Goal:** the exit rule exists from the start, so no new command takes the old shape.
- **Done when:** a syn rule in `crates/er-lint-gate/src/` walks every `#[tauri::command]` function in er-desktop and fails one whose return type is `Result<_, String>` or has a path segment ending in `Snapshot`, resolving local type aliases in the same file; `[workspace.metadata.lint-gate]` lists today's 200 names as the allow-list, and a test fails if the list grows; the rule runs in both clippy passes.
- **Planted breaks:** a new command returning `Result<AppSnapshot, String>` outside the list fails; one returning `Result<u8, String>` fails; one returning an alias `type R = Result<u8, String>` fails; adding a name to the list fails the shrink test.
- Size: S.

### P52.1 The resource host and the three commands

- Branch: `epic/p52a-resource-host`. Lanes: D, S, P. Model: `opus`. Depends on: P50, P51.
- **Goal:** with the flag on, the desktop answers `er_send`, `er_fetch` and `er_versions` and pushes `er://resources`; nothing reads it yet.
- **Done when:** a desktop resource host maps each `ResourceKey` to its owner (workspace actors through `ServiceState`, the registry, the settings actor, the desktop owners); a relay subscribes to each actor's `ViewPublished` and to `TabsChanged`, compares section revisions with the last published view and emits `ResourceChanged` only for keys whose revision moved, coalesced as `run_revision_watcher` does; the three commands are registered, in `permissions/app-commands.toml` and in `ipcAllowlist.test.ts`, and pass `er::command_shape` with no allow-list entry; the first resource, `tabs`, is served; the old `er://revision` path is untouched.
- **Planted breaks:** a relay that emits every key on every publish fails the precision test (an arena tick moves no `diff(tab)` version); a write that does not bump fails the bump-on-write test; an `er_fetch` that blocks on the actor fails the held-actor bound test (ADR 0067); dropping the command from `app-commands.toml` fails the allowlist test.
- **Fallback split:** P52.1a host and relay; P52.1b the commands and permissions.
- Size: M.

### P52.2 The frontend resource client, the adapter pattern and ADR 0072

- Branch: `epic/p52b-resource-client`. Lanes: U, P. Model: `opus`. Depends on: P52.1.
- **Goal:** desktop-ui can hold resources by version, refetch on push, and run one adapter store in both flag modes.
- **Done when:** a hand-typed client wraps the three commands with the generated types; a resource store tracks the versions it holds, refetches a shown key only when an `er://resources` version exceeds it, and asks `er_versions` on the 30 s safety net and once after the listener attaches; the `tabs` adapter store reads `app.snapshot.tabs` with the flag off and `er_fetch` with it on, and the tab strip reads the adapter; ADR 0072 is added; `CONTEXT.md` gains **Resource** (a versioned piece of published state with one writer).
- **Planted breaks:** a store that refetches on every event fails the version test; a safety net that calls the old `poll` with the flag on fails the flag test; a listener attached after a push leaves the store stale, and the attach-time `er_versions` test catches it when that call is removed; the tab strip reading `app.snapshot` directly fails a source scan.
- Size: M.

### P53 Settings, session chrome and panels

- Branch: `epic/p53-settings-resources`. Lanes: S, D, U, P. Model: `opus`. Depends on: P52.2.
- **Goal:** the settings page, the AI picker and the app chrome read resources.
- **Done when:** `settings`, `risk(key)`, `session` and `panels` are served and their adapters read them; the settings writes, the AI selection writes and the risk reads go through `er_send`/`er_fetch` with the flag on; notices arrive on `er://notice`; the entry stops reading 14 `AppSnapshot` fields and 21 commands, listed in the issue.
- **Planted breaks:** a settings write that skips the actor fails the one-writer test (ADR 0069); a refused save whose rollback does not bump the version leaves the page on the refused value and fails the rollback test; a notice delivered twice fails the dedupe test.
- Size: M.

### P54 Tabs and the tab lifecycle

- Branch: `epic/p54-tabs-resource`. Lanes: S, D, U, P. Model: `opus`. Depends on: P53; #578 and P47.2c.2 merged.
- **Goal:** tab strip, opens and closes run on the `tabs` resource and `er_send`, and the phase-4 lifecycle gaps close.
- **Done when:** the tab commands and the opens (19 commands) send through `er_send` with the flag on; `tabs` carries labels and kinds, and staleness is derived in the frontend; a drop of a never-loaded key moves no version; a `LoadTab` for a key the registry no longer lists is refused; the closed-key set is bounded; an all-broken restore leaves a Working tab on the launch root; the launch root comes from the registry; Cmd-T on a remote PR tab opens a Working tab with a root; an already-open PR is selected whatever its tab kind (A15); the entry stops reading `tabs`, `active_tab`, `browser` and `input_mode`.
- **Planted breaks:** a `DropTab` on a never-loaded key that bumps fails the version test; a late `LoadTab` after a close that applies fails the race test; an unbounded closed set fails the bound test after 10,000 closes; `same_pr` comparing kinds fails the cross-kind open test; restoring with every descriptor broken leaves no tab and fails.
- **Fallback split:** P54.1 the resource, commands and adapters; P54.2 the lifecycle fixes.
- Size: L.

### P55.1 The sidebar owner, `projects` and `branches(project)`

- Branch: `epic/p55a-sidebar-projects`. Lanes: DS, US, P. Model: `opus`. Depends on: P52.2.
- **Goal:** the desktop app owner exists and publishes the project rows and each project's branches.
- **Done when:** the owner module holds one writer per resource and bumps on write; `projects` carries the project records without PR lists, and `branches(project)` the local and auto branches with worktree paths and the open PR number, from the metadata cache; `is_active` and `is_current` are derived in the frontend from `tabs`; the project commands (7) go through `er_send`; `auto_register` takes the registry's tabs, not the startup tab; the entry stops reading `projects` (its non-PR fields) and `worktrees`.
- **Planted breaks:** a second writer of the projects record fails the one-writer test; a branch refresh that does not bump `branches(project)` fails; an `is_active` computed on the server fails a source scan; `auto_register` reading the App's startup tab fails the flag-on test.
- Size: M.

### P55.2 `prs(remote)` keyed on content, `head_oid` included

- Branch: `epic/p55b-sidebar-prs`. Lanes: DS, US, P. Model: `opus`. Depends on: P55.1.
- **Goal:** the sidebar PR lists come from one resource per remote whose version follows its content.
- **Done when:** `prs(remote)` carries the PR rows plus server-built id lists for my PRs, to review, recently merged and saved, with the dismissed filter applied on the server; the writer bumps the version whenever the rows change, any field including `head_oid`, so no length-and-time fingerprint decides it; `patch_pr_head_oid` and the full sweep both write through the owner; the head probe takes its target from the `tabs` resource; `detected_pr_number` and the tab staleness are derived in the frontend from these rows; the PR commands (9) go through `er_send`; the saved-PR cap moves to the server.
- **Planted breaks:** a test drives `patch_pr_head_oid` on a cached row and asserts the version moved and the fetched row carries the new oid; planting a version computed from row counts and fetch times (the `build_projects_cache_key` shape) fails it; a dismiss that does not filter on the server fails the dismissed test; a 51st save that lands fails the cap test.
- Size: M.

### P55.3 `inbox` and the global optimistic ops

- Branch: `epic/p55c-sidebar-inbox`. Lanes: DS, US, U, P. Model: `opus`. Depends on: P55.2.
- **Goal:** the inbox is a resource, and the four global optimistic ops are gone with the flag on.
- **Done when:** `inbox` carries the items, the unread count and the last refresh, computed on the server; the inbox commands (9, including `open_inbox_item` and `prefetch_inbox_item`) go through `er_send`; with the flag on, `inbox-read`, `inbox-clear`, `saved-pr` and `dismissed-pr` no longer paint, and the push lands within the A9 budget for mark read and save PR; the entry stops reading `inbox_items`, `inbox_unread_count` and `inbox_last_refresh_ms`.
- **Planted breaks:** an unread count computed in TypeScript fails a source scan; a mark-read that does not bump `inbox` fails; a global op still painted with the flag on fails the flag test; a budget overrun fails the measurement.
- Size: M.

### P56.1 `diff(tab)`, loading on activation and the warmer

- Branch: `epic/p56a-diff-resource`. Lanes: S, D, U, P. Model: `opus`. Depends on: P54.
- **Goal:** the file list, mode, base and staleness of the active tab come from `diff(tab)`.
- **Done when:** `diff(tab)` carries each file's row and content hash and the diff-level fields; fetching it for a key with no content sends `LoadTab`; the warmer loads unloaded registry tabs at idle through `LoadTab`, scoped by the registry; the refresh, mode, base, filter and sync commands (8) go through `er_send`; the visible-file list uses the pure `visible_files` (#595); the entry stops reading 17 fields, listed in the issue.
- **Planted breaks:** a followed stub that never loads fails the first-diff test; a warmer that loads the active tab twice fails the warmer test; a filter computed against the App's tab fails the source scan; a stale pill that ignores `preview_head_oid` fails the staleness test.
- Size: L.

### P56.2 Content-addressed hunks, previews and file reads

- Branch: `epic/p56b-content-addressed-hunks`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.1.
- **Goal:** hunks are fetched by content hash and cached in the frontend; the server records nothing about what it sent.
- **Done when:** `hunks(hash)` is served per A4; DiffView joins hunks with threads from `annotations(tab)` (adapter-fed until P58.1); the frontend cache is an LRU bounded by bytes; previews, compaction expansion, mtime sort and `list_diff_paths` (5 commands) run on the service; with the flag on, no code path calls `record_sent_file` or reads `hunks_omitted`.
- **Planted breaks:** a hash that includes threads makes a comment re-send the file and fails the comment test; an unbounded cache fails the byte-bound test; a fetch of an unparsed file that parses on the reader thread instead of through `ParseFiles` fails the one-parse-path test; a flag-on call to `record_sent_file` fails the source scan.
- **Fallback split:** P56.2a hunks and the cache; P56.2b previews and file reads.
- Size: L.

### P56.3 History, Conflicts and Hidden in the service

- Branch: `epic/p56c-history-modes`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.2.
- **Goal:** the three modes that load on the App with the flag on load in the Workspace.
- **Done when:** the History commit list and the selected commit's files are a section of `TabContent` and part of `diff(tab)`; Conflicts and Hidden load by the engine steps the App runs, extracted into the always-compiled half as 2a.1 did; `select_commit` and `exit_history` go through `er_send`; a History file click no longer reaches the App; the parity test covers the three modes; the entry stops reading `commits` and `selected_commit_sha`.
- **Planted breaks:** skipping the commit list load fails the History parity row; a click routed to `history_select_file` with the flag on fails the source scan; a Conflicts load that reads the App fails the actor-thread scan.
- **Fallback split:** P56.3a the engine extraction (flag-off byte-identical); P56.3b the service and the routes.
- Size: L.

### P57 `reviewed(tab)` and frontend navigation

- Branch: `epic/p57-reviewed-navigation`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.2.
- **Goal:** reviewed marks are a resource, reviewed writes name their tab, and navigation runs in the frontend.
- **Done when:** `reviewed(tab)` carries the marks and the counts; the reviewed commands carry the `TabKey` the page rendered and are refused when it is not the content's key; the checkmark is pending until ack (A8); selection, cursor and `next_file`, `prev_file`, `next_hunk`, `prev_hunk` and `jump_to_unreviewed` run in desktop-ui over the two resources (A10), checked by the shared fixture against the Rust functions; mark reviewed and next file stay within the A9 budget; the entry stops reading `reviewed_count`, `selected_file` and `current_hunk` and 12 commands.
- **Planted breaks:** a reviewed write without the key lands on the registry's other tab and fails the lag test; `reviewed_count` computed in TypeScript fails the source scan; a TypeScript `jump_to_unreviewed` that skips the last file fails the shared fixture; a budget overrun fails the measurement.
- **Fallback split:** P57.1 reviewed; P57.2 navigation.
- Size: L.

### P58.1 `annotations(tab)` and `ui_annotations(tab)`

- Branch: `epic/p58a-annotations-read`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.2.
- **Goal:** findings, threads, tour, checklist, triage, diagrams and the other bucket are read from resources.
- **Done when:** `annotations(tab)` is built from the AI section with thread ordering and counts on the server; `ui_annotations(tab)` is served by the desktop and bumped by its writes; the sidecar reload watches the comments directory as well as `er_dir`; the four read commands go through `er_fetch`; the entry stops reading `ai`, `tour`, `other_bucket` and `ui_annotations`.
- **Planted breaks:** an outside write to `questions.json` that never reaches the content fails the reload test when the comments directory is dropped from the watch; thread ordering done in TypeScript fails the source scan; an arena tick that bumps `annotations(tab)` fails the precision test.
- Size: L.

### P58.2 Annotation writes and pending until ack

- Branch: `epic/p58b-annotation-writes`. Lanes: D, U, P. Model: `opus`. Depends on: P58.1, P55.3 (both edit `optimisticLocal.ts`).
- **Goal:** the 27 annotation write commands go through `er_send` and the thread paints shrink to pending until ack.
- **Done when:** each write maps to the service command P41 added, by `TabKey`; creation paints pending with the `client_id` and the confirming fetch lands on the same row (ADR 0017); every other thread paint is deleted with the flag on; add question stays within the A9 budget.
- **Planted breaks:** dropping `client_id` makes the confirmed row get a fresh id and fails the confirm test; a resolve still painted with the flag on fails the flag test; a budget overrun fails the measurement.
- **Fallback split:** P58.2a threads, comments and promote; P58.2b findings, checklist, artifacts and browser annotations.
- Size: L.

### P59 `pr(tab)`, `pr_status`, the stack and the GitHub senders

- Branch: `epic/p59-pr-resources`. Lanes: S, D, DS, U, P. Model: `opus`. Depends on: P58.1, P55.2.
- **Goal:** the PR card, merge box, GitHub status and stack read resources, and no GitHub command picks its tab from the App.
- **Done when:** `pr(tab)` is built from the PR section; `pr_status(remote, number)` from the status cache through the desktop owner; the stack is part of `pr(tab)` or its own resource, decided in the step plan; `active_tab_command`, `send_for_active_tab`, the comment tick, `abort_wrong_view` and the document preview check take the tab from the `tabs` resource and its content; the 13 GitHub commands go through `er_send`; the entry stops reading `pr`, `github` and `stack`.
- **Planted breaks:** a GitHub post that reads the App's active tab fails the held-App-mutex test; a status refresh that does not bump `pr_status` fails; a merge pinned to a stale head fails the head test.
- Size: L.

### P60.1 Arena and review flows on `jobs`

- Branch: `epic/p60a-jobs-arena`. Lanes: S, D, U, P. Model: `opus`. Depends on: P54. Runs beside P59 (planned pair).
- **Goal:** the arena page and the flow strip read `jobs`, and arena and flow commands stop resolving their tab on the App.
- **Done when:** `jobs` carries the live arena runs, the review flows and their progress across actors; arena `resolve` and flow `key` take the tab from the `tabs` resource; the 20 arena and flow commands go through `er_send` and their reads through `er_fetch`; `App.svelte:293`'s `er://revision` listener is replaced by the `jobs` resource; the entry stops reading `review_flow`, `active_arena_run`, `arena_runs` and `background_arena_runs`.
- **Planted breaks:** an arena start that resolves the App's tab fails the held-App-mutex test; a progress tick that does not bump `jobs` fails; the Deep cancel and orphan test from P28b/P32b is reused unchanged.
- Size: L.

### P60.2 AI runs, triage and task logs on `jobs`

- Branch: `epic/p60b-jobs-runs`. Lanes: S, D, DS, U, P. Model: `opus`. Depends on: P60.1.
- **Goal:** card AI, review runs, triage and the background task panel read `jobs`.
- **Done when:** the 19 run, triage and log commands go through `er_send`/`er_fetch`; triage spinners read `jobs` (owner answer); auto-triage writes its queue through the desktop owner; the entry stops reading `agent_commands`, `agent_log`, `background_tasks` and `bg_loading`.
- **Planted breaks:** a triage queued without a `jobs` bump leaves the spinner off and fails; a task log fetched from the App fails the source scan; a cancel that does not reach the runner fails the reused `CancelJob` assertion (P43).
- Size: L.

### P61 Window-mechanics commands typed

- Branch: `epic/p61-window-commands`. Lanes: D, U, P. Model: `sonnet`. Depends on: P51, P52.1. Runs in the sidebar track after P55.3.
- **Goal:** the 27 commands that stay direct return `Result<T, ErError>` with protocol types.
- **Done when:** the browser webview (9), terminal (4), OS launch and reveal (6), export (3), `start_window_drag`, `detect_dev_url`, `line_commits`, `fetch_github_image` and `dev_log_filter` commands are typed; their names leave the allow-list; their callers use the generated types.
- **Planted breaks:** restoring one to `Result<_, String>` fails `er::command_shape`; re-adding a name to the allow-list fails the shrink test.
- Size: M.

### P62.1 Delete the old store layer

- Branch: `epic/p62a-delete-old-store-layer`. Lanes: U, P, C. Model: `opus`. Depends on: P53–P61.
- **Goal:** desktop-ui runs only on resources.
- **Done when:** the adapters lose their flag-off branch; the `snapshot` store, `poll` handling, `optimisticLocal.ts`'s remainder, `snapshotDelta.ts`, `snapshotChrome.ts`, `tabSnapshotCache.ts` and `v1ServiceFlag.ts` are deleted; the hand-written `types.ts` is replaced by a generated barrel that `protocol-check` covers, and frontend-only types move to their modules; `scripts/tauri-dev.sh` drops the flag; er-desktop makes `v1-service` a default feature for the one release step between P62.1 and P62.2.
- **Planted breaks:** a hand edit of `types.ts` fails `protocol-check`; a component reading `app.snapshot` fails the source scan; `check-ui` fails on a removed field still referenced.
- Size: M (deletions do not count).

### P62.2 Delete the old desktop path and the desktop flag

- Branch: `epic/p62b-delete-old-desktop-path`. Lanes: D, DS, C, P. Model: `opus`. Depends on: P62.1.
- **Goal:** the phase exit.
- **Done when:** `snapshot.rs` and `commands.rs` are deleted, with the App-backed desktop route, `get_snapshot`, `poll`, `v1_service_enabled`, `run_revision_watcher` and the follower; er-desktop has no `er_engine::app` reference and depends on er-engine with `default-features = false` (`export.rs` moves off the `ui`-gated `er_engine::export` first); the `v1-service` feature leaves er-desktop and stays on er-tui (A12), and `test-v1`, CI and the lint gate's second pass keep only er-tui; the `er::command_shape` allow-list is empty and deleted, and no command returns a `*Snapshot` type or `Result<_, String>`; ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034 are marked "Superseded by ADR 0072", and docs-check fails when a superseded mark and the superseding ADR's list disagree; `CLAUDE.md`'s push-driven line, `crates/er-desktop/agent.md`, `desktop-ui/agent.md` and `desktop-ui/src/lib/stores/agent.md` describe resources; `app-commands.toml` and `ipcAllowlist.test.ts` list the surviving commands only.
- **Planted breaks:** a command returning `Result<AppSnapshot, String>` fails `er::command_shape` with no allow-list to hide in; a `use er_engine::app::App` in er-desktop fails the build; deleting 0012 from ADR 0072's supersedes line fails docs-check; a compile error behind `cfg(feature = "v1-service")` in er-tui still fails CI on a throwaway draft PR (#470's shape); re-adding the feature to er-desktop fails the default-build `cfg!` test, moved to assert the feature is gone.
- **Fallback split:** P62.2a the deletion and the crate edges; P62.2b the rule, the ADR marks, docs-check and CI.
- Size: M (deletions do not count).

## Lanes and schedule

| Lane | Hot files |
|---|---|
| **C** crates, gate, CI | root `Cargo.toml`/`Cargo.lock`, `justfile`, `.github/workflows/ci.yml`, `scripts/**`, `crates/er-lint-gate/**`, `crates/er-desktop/Cargo.toml` |
| **P** protocol | `crates/er-protocol/**`, `desktop-ui/src/lib/protocol/**` (generated part regenerated on conflict) |
| **S** service | `crates/er-service/**`, and the engine's always-compiled half when a resource needs an extracted step |
| **D** desktop backend | `crates/er-desktop/src/**` except the DS files, `crates/er-desktop/permissions/**` |
| **DS** desktop sidebar | `projects.rs`, `pr_cache.rs`, `inbox.rs`, `gh_status_cache.rs`, `auto_triage.rs` and the app owner module in er-desktop |
| **U** desktop-ui | `desktop-ui/src/**` except the US files |
| **US** sidebar UI | `LeftSidebar.svelte`, `InboxView.svelte`, `InboxPanel.svelte`, `sidebarModel.ts`, `sidebarPrOpen.ts`, `inbox*.ts`, `projectOrder.ts` and the sidebar adapter stores |

- **Chain:** P50 → P51 → P52.1 → P52.2, then two tracks:
  - workspace: P53 → P54 → P56.1 → P56.2 → { P56.3, P57, P58.1 } in that
    order → P58.2 → { P59 ∥ P60.1 } → P60.2;
  - sidebar: P55.1 → P55.2 → P55.3 → P61.
  - Both tracks join at P62.1 → P62.2.
- **Pair (a), the two tracks.** Their files are disjoint except P (new
  `ResourceKey` variants and generated files) and the resource host's dispatch
  in D, which both extend. Those are conflict-by-addition; generated files are
  regenerated. P55.3 holds U for its `optimisticLocal.ts` and `app.svelte.ts`
  edits, so it waits for the workspace entry holding U to merge, and P58.2
  waits for P55.3.
- **Pair (b), P59 beside P60.1.** P59 edits the GitHub senders and the PR
  card; P60.1 edits `arena_service.rs`, `flow_service.rs`,
  `arena_commands.rs` and `stores/arena.svelte.ts`. Shared files:
  `ResourceKey`, the host dispatch, the wire command enum. Additions only.
- At most three lanes run at once (PLAN); the sidebar track holds DS and US,
  which the workspace track never takes, so the two tracks fit beside each
  other.

## Risks

1. **Latency the optimistic layer hid.** Each entry that deletes a paint
   measures its action against A9's budget; an overrun keeps that one paint as
   pending until ack, recorded in the PR.
2. **Two render paths until P62.** Every adapter store has a flag-off and a
   flag-on branch; `test-ui` runs both, and `test-v1` runs the backend half.
   The coordinator stops new adapter work if a flag-off regression lands.
3. **Wire types grow beside internal types.** er-protocol re-describes what
   `snapshot.rs` maps today; the builders are where drift hides. Each resource
   entry carries a parity test of its builder against the flag-off snapshot
   fields it replaces.
4. **IPC fan-out.** Hunks fetched per file could multiply calls; `er_fetch` is
   batched and DiffView requests the visible window's hashes in one call.
5. **v0.6.x fixes land on the flag-off path** (D2). The gate merge brings
   them; the coordinator ports any fix to `snapshot.rs` or `commands.rs` into
   the resource builder of the entry that owns that field.
6. **The follower lag window stays open until P62.2.** P57 closes it for
   reviewed writes and #578 for annotations; the rest close by deletion.
7. **The sidebar owner moves again in phase 6.** A13's trait keeps that move
   inside the desktop and the service.
8. **History in the service is new extraction work.** P56.3 carries a split
   and a parity test, as 2a.1 did.
9. **Machine load.** Two tracks at most, each heavy check through the lock.

## ADR and doc changes

- P52.2 adds ADR 0072 (the protocol) and the **Resource** term in
  `CONTEXT.md`.
- P62.2 marks ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034 superseded by
  0072, adds the two-way supersession check to docs-check, and rewrites the
  push-driven line in `CLAUDE.md` and the poll text in
  `crates/er-desktop/agent.md`, `desktop-ui/agent.md` and
  `desktop-ui/src/lib/stores/agent.md`.
- ADR 0015's flag-path paragraph and ADR 0067's "routed open commands return
  an empty snapshot" consequence are updated in P62.2.
- ADR 0068 gains a consequence in P57: selection and navigation live in
  desktop-ui; the Rust navigation functions stay for the TUI (D20).

## Notes

- Field and command lists live in each issue body (ADDENDUM-03). The
  assignment covers all 61 `AppSnapshot` fields and all 205 command functions
  exactly once; P62.2 deletes `get_snapshot`, `poll` and
  `v1_service_enabled`.
- **Owner question carried from phase 4:** `build_projects_cache_key`'s
  `head_oid` gap is live on `main` (v0.6.x). Bug fixes go to `main`
  (CLAUDE.md); P55.2 fixes the resource path only.
- **Re-pin** marks: #578 (`88c21f507`) and P47.2c.2 (`d5e060baa`). The
  coordinator confirms the "left on the App" list from P47.2c.2's PR body.
- Phase 5b and phase 6 are out of scope: the TUI keeps the `App`,
  `navigation.rs`'s `App` methods and the `TabState` view fields (D20);
  inbox, projects, MCP and the CLI move in phase 6.
