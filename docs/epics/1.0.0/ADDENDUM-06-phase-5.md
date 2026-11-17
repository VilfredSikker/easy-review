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
sites are cited at those heads and marked **re-pin**. After the pin, #578
merged at `5942bc925` and P47.2c.2's local head moved to `2f709c637`; once
P47.2c.2 merges too, every
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
  one each in `crates/er-desktop/src/commands/line_commits.rs`, `gh_pr_actions.rs`, `github_image.rs`
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
  types, plus `desktop-ui/src/lib/types/arena.ts` (233). `app.svelte.ts` (1,275 lines) holds the
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
  carry the merge and cache rules. 104 non-test `.ts` and `.svelte` files
  (stories excluded) read `app.snapshot` or name `AppSnapshot`.
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
  `crates/er-engine/src/lib.rs`; `arena` and `navigation` are always compiled.
  er-desktop depends on er-engine with default features and names
  `er_engine::app::` on 158 lines (98 in `commands.rs`). Its `export.rs` calls
  the `ui`-gated `er_engine::export`.
- **No codegen exists.** No crate depends on `ts-rs` or `specta`.
- **The sidebar's PR fingerprint ignores `head_oid`.** `build_projects`
  (`snapshot.rs:4552`) memoizes the project rows behind `ProjectsCacheKey`
  (`:4581`); `build_projects_cache_key` (`:4592`) hashes each remote's PR count
  and last fetch time. The 30 s head probe (the thread at `main.rs:1316`) calls
  `patch_pr_head_oid` (`pr_cache.rs:114`, call `main.rs:1362`), which changes
  one row's `head_oid` in place and touches neither, so the sidebar keeps the
  old oid until the next full sweep. The coordinator's "about line 4484" was a
  `main` line number; at this ref it is `:4592`. The probe also picks its
  target from the App's active tab (`pr_head_probe_target`, `main.rs:656`).
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
  startup tab on the flag path → **P54** and **P56.1**. The startup
  registration block (`main.rs:997`–`:1003`: `sync_projects_from_tabs(&app.tabs)`,
  `projects.rs:472`, then `auto_register` on `app.tab().repo_root`) →
  **P55.1**.
- `toggle_reviewed` and `mark_reviewed` (`write_reviewed`,
  `content_routes.rs:372`) have a follower-lag gap → **P57**: the command
  carries the `TabKey` the page rendered. P62.2 deletes the follower, which
  closes the class.
- The sidecar reload watches `er_dir` only (#596; `readers`,
  `er-service/src/sidecar_reload.rs:115`) → **P58.1**.
- Only the frontend's `poll` drives `poll_impl` (`commands.rs:11869`, its one
  caller `app.svelte.ts:589`), and its first pass is the only trigger for
  `content_routes::reload_sidecars` (`commands/content_routes.rs:263`, active
  tab only), `drain_agent_log`, `check_commands`, `poll_background_tasks`,
  `process_ai_task_inbox`, `check_ai_files_changed` and
  `kick_worktrees_warm` → **P52.1** (the backend tick, A17).
- Review follow-ups from the phase-4 PRs:
  - #577 plants 8 and 9 survive because no remote-PR-tab arena or flow test
    exists → **P60.1** (an arena and a flow test on a remote PR tab, keyed
    through `tabs`).
  - #595 note 1: `history_commit` stays set outside History, so the App's
    preview key differs from the service's (`er-service/src/content.rs:77`
    passes `history_commit: None`) → **P56.3**, with the preview-stamp parity.
  - #596: a stamp read after the AI read in `load_tab` can lose an outside
    write → **P58.1**.
  - #596: a spawn failure in `SidecarReloads::start`
    (`er-service/src/sidecar_reload.rs:40`) leaves the key in `running`
    (`:54`), so every later reload answers false → **P58.1**.
  - #596: `LoadTab` in History, Conflicts or Hidden still fails, and recorded
    modes keep the old view's context and AI bucket → **P56.3**.
  - #578: the gating scan accepts `cfg(any(feature = "v1-service", test))`;
    the remote-PR row check "replaced tab's content stayed" in `every_kind…`
    cannot fail; m02 (stubs built outside the App lock) is untested →
    **deferred to P62.2**: #578 records all three as closing when the
    follower is deleted, and P62.2 deletes it; a fix before then would test
    code that P62.2 removes.
- Cmd-T on a remote PR tab opens a rootless Working tab (predates phase 4) →
  **P54**.
- **"Left on the App with the flag on"** (provisional; the coordinator
  confirms the final list from P47.2c.2 before approval):
  - rendering: `get_snapshot` and `poll` build every `AppSnapshot` section
    through `AppRead` → P53–P60 port each section; P62.2 deletes both;
  - History, Conflicts and Hidden load on the App (`commands.rs:1333`), and
    History file clicks reach `history_select_file` (P47.2c.2) → **P56.3**;
  - compaction expansion (`toggle_compacted`), file and image previews and
    `list_diff_paths` → **P56.2**;
  - GitHub senders keyed off the App tab: `active_tab_command`
    (`commands.rs:6836`), `send_for_active_tab` (`:6845`) and the comment
    tick, plus `abort_wrong_view` (`:343`) and the document preview check →
    **P59**;
  - arena `resolve` (`arena_service.rs:55`) and flow `key`
    (`flow_service.rs:39`) → **P60.1**;
  - the PR head probe's target (`pr_head_probe_target`) → **P55.2**;
  - background threads in `main.rs` that read the App's active tab
    (all at `949eb993d`):
    - the active-branch watcher `run_active_branch_watcher` (`:702`) picks
      its target with `desired_local_branch_watch` (`:719`, `:2245`),
      mirrors the checkout root with `apply_watch_checkout_root` (`:753`),
      and on a file change calls `refresh_diff_quick_with_unmark` on the App
      tab (`:783`), so with the flag on `diff(tab)` never follows a file save
      and auto-unmark never reaches the reviewed section → **P56.1**;
    - the branch-base remote-oid probe (`:1234`, `probe_app.try_lock()`
      `:1239`), which feeds the stale pill for branch views → **P56.1**;
    - the GitHub status loop (`:1386`, `active_pr_identity(g.tab(), …)`
      `:1395`), which feeds `pr_status` → **P59**;
    - the inbox sweep's prefs (`commands::clone_inbox_prefs(&bg_app)` `:1609`,
      `:1642`; `commands.rs:465` reads `app.config.inbox`) → **P55.3**;
    - the meta-cache interval (`meta_app.try_lock()…tab().is_remote()`
      `:1708`) → **P55.1**;
  - the App follower (`tabs::follow_registry`, #578) → deleted in **P62.2**.

## Gate steps (coordinator, before P50 starts)

1. #578 and P47.2c.2 merged and #538 closed with the exit checklist and the
   "left on the App" list in its body; STATUS and BUILD_LOG updated.
2. **D2:** merge `main` into `release/v1.0.0` (three CI commits at
   `f107a0944`) and run `just ci` through the lock. The coordinator does this
   at the gate; no entry holds it.
3. Re-pin every **re-pin** citation here and in the step plans at the merge
   head; record what moved in this file's Notes.
4. The coordinator confirms the proposals below and the owner approves the
   plan; then the issues open as sub-issues of #371.
5. The phase-5 gate is logged in BUILD_LOG (PLAN's gate step 6).

## Decisions settled by the coordinator

Each is a planning proposal the coordinator confirms or changes at the gate.

- **A1 Crate shape.** `er-protocol` is a leaf crate of wire types: serde,
  `ts-rs`, and nothing from the workspace. A second dependency allow-list in
  the existing `er::core_io` rule, beside er-core's `ALLOWED`
  (`crates/er-lint-gate/src/core_io.rs:25`), holds it there; the rule checks
  er-protocol's manifest only, not its source. Deriving `TS` on er-core or
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
  frontend branches on. The conversion is `impl From<WorkspaceError> for
  ErError` in er-service, beside `WorkspaceError` (`er-service/src/handle.rs:25`):
  er-service gains its er-protocol dependency in P52.1, which A1's builders
  need anyway. er-desktop cannot hold that impl, since the trait and both
  types would be foreign to it (E0117). The wire command enum lives in
  er-protocol and names tabs by `TabKey`; the desktop adapter resolves the
  `TabContext` from the published content, as the phase-4 content routes do.
  Every flag-on command today has a flag-off twin, so the three commands are
  registered in both builds; the flag-off bodies answer
  `ErErrorKind::Unsupported` and go in P62.2.
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
  - Notices become an `er://notice` event: a notice happens once and has
    nothing to refetch.
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
  allow-list freezes the 196 names that fail it today and may only shrink:
  a name outside the frozen set fails, and so does a stale entry, one with no
  command or whose every body passes. P61 shrinks it and P62.2 deletes it. Planted breaks cover the two direct shapes and a type alias
  (`type R = Result<X, String>`), which a literal match would miss.
- **A7 The protocol ADR.** P52.2 adds the protocol ADR, numbered 0072 unless that number is
  taken at commit time: resources with versions, push invalidation per key,
  content-addressed hunks, `ErError`, the three commands, `ts-rs`, and
  ADR 0017's `client_id` rule carried over unchanged. The seven
  "Superseded by" marks on ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034
  land in P62.2, because the flag-off path runs under them until then.
- **A8 The optimistic layer.** Each of the 22 `OptimisticOp` kinds
  (`optimisticLocal.ts:76`) and the reviewed path is classified in the entry
  that ports the commands that paint it, and that entry stops the paint with
  the flag on, so no paint waits for a confirming `AppSnapshot` that an `Ack`
  never carries. The owners: `saved-pr` and `dismissed-pr` in P55.2
  (`save_pr`, `unsave_pr`, `dismiss_remote_pr`, `undismiss_remote_pr`);
  `inbox-read` and `inbox-clear` in P55.3; `bulk-reviewed` in P57
  (`bulk_review_pillar`, `unbulk_review_pillar`); the other 17 in P58.2,
  each named there. 2 + 2 + 1 + 17 = 22. The classes:
  - **pending until ack** (ADR 0017's `client_id`): thread and comment
    creation (`add-thread`, `reply`, `promote`, `promote-finding`, and
    `reply-finding` for its `existing` and `new` targets; its `ai` target
    paints nothing today) and the reviewed checkmark on the one clicked path;
  - **rule moved to the server**: the saved-PR cap (`MAX_SAVED_PRS`
    `optimisticLocal.ts:74`; the server keeps 50 by dropping the oldest, as
    `projects.rs:190`–`:199` does), the dismissed-PR filter, inbox unread
    counts, `reviewed_count` arithmetic and thread ordering are computed by
    the resource builder, and the paint that applied them is deleted;
  - **deleted**: every other paint, once its resource's push arrives. A hot
    action (A9) whose measured median overruns the budget keeps its paint as
    pending until ack, recorded in the PR.
- **A9 Latency budget.** Old: the flag-off command plus
  `build_snapshot_with_delta` plus serialisation of the returned
  `AppSnapshot`. New: `er_send` to `Ack`, plus the changed resources'
  `er_fetch` and their serialisation. Budget: within 10% or 2 ms of the old
  median, whichever is larger (ADDENDUM-05, B6). These are reported
  measurements, as in P47.1: the harnesses are `#[ignore]` and never a CI
  assertion (`er-service/src/tests.rs:208`, `content_tests.rs:463`), so each
  entry records the old and new medians in its PR, and an overrun is acted on
  there (the paint stays pending until ack, or the entry does not merge
  until the coordinator accepts the number). P52.1 extends the harness to
  er-desktop, with the old-path baseline taken over N runs on a 5,000-line
  diff before any store moves. The hot actions and the entry that measures
  each: open target in P54; save PR in P55.2; mark read in P55.3; tab switch
  in P56.2, because hash-keyed hunks are what change its cost and they land
  there; mark reviewed and next file in P57; add question in P58.2. An
  action whose paint was optimistic is also timed click-to-paint in the
  desktop with `profileLog`, and the numbers go in the PR.
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
  (`desktop-ui/src/lib/protocol/generated/`) and, from P62.1, the generated
  `types.ts` barrel are excluded from ESLint with a reason, because a hand fix
  would be overwritten; `check-ui` still type-checks them and
  `protocol-check` covers both. On a merge conflict they are regenerated,
  never hand-merged, as with the clippy budget file. Export runs only through
  an explicit generator that calls ts-rs's export on each type; no type
  carries `#[ts(export)]`, so `cargo test --workspace` never rewrites the
  committed files and the drift check does not depend on what ran first.
  The barrel is written by the same generator: an index that re-exports the
  generated types and nothing else. Two checks hold the "no
  `#[ts(export)]`" rule, because ts-rs's export test writes to
  `TS_RS_EXPORT_DIR` or to `./bindings` under the crate, an untracked path
  that a diff of the generated directory never sees: a unit test in
  er-protocol fails on any `ts(export` attribute in its source, and CI's
  `test` job fails when `git status --porcelain` is not empty after
  `cargo test`.
- **A17 The backend tick.** `poll_impl` (`commands.rs:11869`) runs only when
  the frontend calls `poll`, which it does on each `er://revision` event and
  on the 30 s safety net. P52.1 adds a flag-on backend tick in er-desktop that
  sends `ReloadSidecars` for every loaded key (today's
  `content_routes::reload_sidecars` covers the active tab only) and runs
  `poll_impl`'s other first-pass duties: `drain_agent_log`,
  `check_commands`, `poll_background_tasks`, `process_ai_task_inbox`,
  `check_ai_files_changed` and `kick_worktrees_warm`. With the flag on,
  `poll_impl` skips that first pass and its `reload_sidecars` call
  (`commands.rs:11873`–`:11874`), so each duty has one driver. The tick keeps
  the old trigger set: it wakes on each revision bump that
  `run_revision_watcher` (`main.rs:855`) debounces, as the duties did when
  every `er://revision` event made the frontend call `poll`, and on an
  interval as the floor. The interval is a proposal: 2 s, the cadence the
  poll had when it was the primary mechanism (`app.svelte.ts:281`–`:284`);
  the reload's stamp check is a `stat` per sidecar, and P52.1 records its
  cost. The snapshot's own `poll`
  keeps running with the flag on for the stores not yet ported, until P62.1
  deletes the snapshot store. Each duty that reads the App leaves the tick in
  the entry that owns its data: `kick_worktrees_warm` takes its target from
  `tabs` in P55.1, `check_ai_files_changed` goes in P58.1 (the annotations
  come from the AI section), and `drain_agent_log`, `check_commands`,
  `poll_background_tasks` and `process_ai_task_inbox` move off the App with
  the runs they drive in P60.2. P62.2 deletes `poll`, after the tick exists.

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
`test-all` (er-service is outside `just test`; er-protocol joins it in P50), `test-v1` and
`build-service-headless` as the touched crates require; `build-mcp` when engine
features or dependencies change; `docs-check` for docs and ADRs; and for every
entry that touches desktop-ui, `check-ui`, `lint-ui` and `test-ui`. Lint only
tightens (ADR 0038). Each entry from P52.1 on runs `protocol-check` (P50).
Until P62, every store entry leaves the desktop working with the flag on and
with it off; `test-ui` runs each adapter store's tests in both modes. The stop
line is 1,500 added-plus-changed lines after rename detection; pure deletions
are measured by what remains and do not count (ADDENDUM-04's L5).

### P50 `er-protocol`, ts-rs codegen and the drift check

- Branch: `epic/p50-er-protocol`. Lanes: C, P. Model: `opus`. Depends on: #538 closed and this addendum's gate steps.
- **Goal:** the wire crate and generated TypeScript, before any resource uses them.
- **Done when:** `crates/er-protocol` exists as a leaf crate with `ResourceKey`, `ResourceVersion`, `Versioned<T>`, `ResourceChanged`, `Ack`, `ErError { kind, message }` and an empty wire command enum, each deriving serde and `TS`; `just protocol-gen` runs the explicit generator (A16) into `desktop-ui/src/lib/protocol/generated/`; `just protocol-check` exports into a temporary directory and fails on any difference with the committed files, and CI runs it on the `rust` and `ui` change outputs; `protocol-check` joins `just ci`; the `er::core_io` manifest check covers er-protocol (serde, ts-rs) through a second allow-list beside er-core's `ALLOWED` (A1); er-protocol joins `just test`; a unit test in er-protocol fails on any `ts(export` attribute in its source, and CI's `test` job fails when `git status --porcelain` is not empty after `cargo test` (A16).
- **Planted breaks:** a hand edit of one generated file fails `protocol-check`; a new field on a protocol struct without regenerating fails it; adding `er-engine` to er-protocol's dependencies fails the allow-list; removing the CI step leaves the hand edit green on a draft PR; adding `#[ts(export)]` to one type fails the source-scan test; with that test skipped, the same plant makes `cargo test` write an untracked file under `crates/er-protocol/bindings/`, and the porcelain step fails on a draft PR.
- Size: M.

### P51 `er::command_shape` with a shrinking allow-list

- Branch: `epic/p51-command-shape-rule`. Lanes: C. Model: `opus`. Depends on: P50 (it names `ErError`).
- **Goal:** the exit rule exists from the start, so no new command takes the old shape.
- **Done when:** a syn rule in `crates/er-lint-gate/src/` walks every `#[tauri::command]` function in er-desktop and accepts it only when its return type is `Result<T, ErError>` or a type that is not a `Result`, and no path segment of the return type ends in `Snapshot`; it resolves local type aliases in the same file; `[workspace.metadata.lint-gate]` lists the 196 names that fail it today as the allow-list; the list only shrinks: a name outside the frozen fixture fails, and a stale entry fails, meaning a listed name with no command, or whose every body passes the rule, as the `comment-boundary` list already does (`Cargo.toml:150`–`:153`); the rule also scans bodies under `cfg(feature = "v1-service")`, since it reads the source with syn and sees both `cfg` arms.
- **Planted breaks:** a new command returning `Result<AppSnapshot, String>` outside the list fails; one returning `Result<u8, String>` fails; one returning an alias `type R = Result<u8, String>` fails; a flag-only command (`#[cfg(feature = "v1-service")]`) with the wrong shape fails; adding a name to the list fails; leaving a name on the list after its command passes fails as stale.
- Size: S.

### P52.1 The resource host, the three commands and the backend tick

- Branch: `epic/p52a-resource-host`. Lanes: D, S, P, U, C. Model: `opus`. Depends on: P50, P51.
- **Goal:** with the flag on, the desktop answers `er_send`, `er_fetch` and `er_versions` and pushes `er://resources`; the backend tick (A17) drives the sidecar reload and `poll_impl`'s other first-pass duties; nothing reads the resources yet.
- **Done when:** a desktop resource host maps each `ResourceKey` to its owner (workspace actors through `ServiceState`, the registry, the settings actor, the desktop owners); a relay subscribes to each actor's `ViewPublished` and to `TabsChanged`, compares section revisions with the last published view and emits `ResourceChanged` only for keys whose revision moved, coalesced as `run_revision_watcher` does; the three commands are registered in both builds (A2), in `crates/er-desktop/permissions/app-commands.toml` and in `ipcAllowlist.test.ts`, and pass `er::command_shape` with no allow-list entry; `impl From<WorkspaceError> for ErError` lives in er-service, which gains its er-protocol dependency (A2); the first resource, `tabs`, is served; the backend tick sends `ReloadSidecars` for every loaded key and runs `poll_impl`'s other first-pass duties, which `poll_impl` skips with the flag on (A17); the latency harness is extended to er-desktop and the old-path medians for A9's hot actions are recorded in the PR as the baseline; the old `er://revision` path is untouched.
- **Planted breaks:** a relay that emits every key on every publish fails the precision test (an arena tick moves no `diff(tab)` version); a write that does not bump fails the bump-on-write test; an `er_fetch` that blocks on the actor fails the held-actor bound test (ADR 0067); dropping the command from `app-commands.toml` fails the allowlist test; with the tick disabled, an outside write to a sidecar in a loaded tab's `er_dir` leaves that tab's `AiSection` revision unmoved with no frontend call, and the reload test fails; a `poll_impl` that still runs its first pass or its `reload_sidecars` call with the flag on fails the one-driver test; a tick that ignores revision bumps leaves a finished agent command undrained until the interval and fails the wake test.
- **Fallback split:** P52.1a host, relay and the tick; P52.1b the commands, permissions and the harness.
- Size: L.

### P52.2 The frontend resource client, the adapter pattern and the protocol ADR

- Branch: `epic/p52b-resource-client`. Lanes: U, P. Model: `opus`. Depends on: P52.1.
- **Goal:** desktop-ui can hold resources by version, refetch on push, and run one adapter store in both flag modes.
- **Done when:** a hand-typed client wraps the three commands with the generated types; a resource store tracks the versions it holds, refetches a shown key only when an `er://resources` version exceeds it, and asks `er_versions` on its own 30 s safety net and once after the listener attaches; the snapshot's `poll` keeps running with the flag on beside it, for the stores not yet ported, until P62.1 (A17); the `tabs` adapter store reads `app.snapshot.tabs` with the flag off and `er_fetch` with it on, and the tab strip reads the adapter; the protocol ADR is added; `CONTEXT.md` gains **Resource** (a versioned piece of published state with one writer).
- **Planted breaks:** a store that refetches on every event fails the version test; a resource store that refreshes through `poll` instead of `er_versions` and `er_fetch` fails the flag test; a listener attached after a push leaves the store stale, and the attach-time `er_versions` test catches it when that call is removed; the tab strip reading `app.snapshot` directly fails a source scan.
- Size: M.

### P53 Settings, session chrome and panels

- Branch: `epic/p53-settings-resources`. Lanes: S, D, U, P. Model: `opus`. Depends on: P52.2.
- **Goal:** the settings page, the AI picker and the app chrome read resources.
- **Done when:** `settings`, `risk(key)`, `session` and `panels` are served and their adapters read them; the settings writes, the AI selection writes and the risk reads go through `er_send`/`er_fetch` with the flag on; notices arrive on `er://notice`; the entry stops reading 14 `AppSnapshot` fields and 21 commands, listed in the issue.
- **Planted breaks:** a settings write that skips the actor fails the one-writer test (ADR 0069); a refused save whose rollback does not bump the version leaves the page on the refused value and fails the rollback test; a notice delivered twice fails the dedupe test.
- **Fallback split:** P53.1 `settings` and `risk(key)` with the settings and risk commands; P53.2 `session`, `panels`, the AI selection writes and `er://notice`.
- Size: M.

### P54 Tabs and the tab lifecycle

- Branch: `epic/p54-tabs-resource`. Lanes: S, D, U, P. Model: `opus`. Depends on: P53, #538 (#578 and P47.2c.2 merged).
- **Goal:** tab strip, opens and closes run on the `tabs` resource and `er_send`, and the phase-4 lifecycle gaps close.
- **Done when:** the tab commands and the opens (19 commands) send through `er_send` with the flag on, and none of them takes the App mutex; `tabs` carries labels and kinds; a tab's staleness keeps the snapshot's value in the adapter until P55.2 derives it from `prs(remote)`; a drop of a never-loaded key moves no version; a `LoadTab` for a key the registry no longer lists is refused; the closed-key set is bounded; an all-broken restore leaves a Working tab on the launch root; the launch root comes from the registry; Cmd-T on a remote PR tab opens a Working tab with a root; an already-open PR is selected whatever its tab kind (A15); the open-target median is recorded against the A9 baseline; the entry stops reading `tabs`, `active_tab`, `browser` and `input_mode`. No desktop-ui component reads `input_mode` (only `snapshotChrome.ts:57` carries it over and `types.ts:793` declares it), so the flag-on desktop drops it and no resource carries it; the key handler's modes stay frontend-local, and the App keeps `input_mode` for 5b (D20).
- **Planted breaks:** a `DropTab` on a never-loaded key that bumps fails the version test; a late `LoadTab` after a close that applies fails the race test; an unbounded closed set fails the bound test after 10,000 closes; `same_pr` comparing kinds fails the cross-kind open test; restoring with every descriptor broken leaves no tab and fails; a `launch_root` that reads the App's startup tab fails the flag-on launch-root test; an open that locks the App fails the held-App-mutex test.
- **Fallback split:** P54.1 the resource, commands and adapters; P54.2 the lifecycle fixes.
- Size: L.

### P55.1 The sidebar owner, `projects` and `branches(project)`

- Branch: `epic/p55a-sidebar-projects`. Lanes: DS, US, P, D (planned exception, see pair (a)). Model: `opus`. Depends on: P52.2.
- **Goal:** the desktop app owner exists and publishes the project rows and each project's branches.
- **Done when:** the owner module holds one writer per resource and bumps on write; `projects` carries the project records without PR lists, and `branches(project)` the local and auto branches with worktree paths and the open PR number, from the metadata cache; `is_active` and `is_current` are derived in the frontend from `tabs`; the project commands (7) go through `er_send`; on the flag path the startup registration block (`main.rs:997`–`:1003`) registers from the registry's tab list instead of `sync_projects_from_tabs(&app.tabs)` (`projects.rs:472`) and `app.tab().repo_root`; the meta-cache loop picks its interval (`main.rs:1708`) from the `tabs` resource's active key instead of the App tab, and the tick's `kick_worktrees_warm` takes its target from the same key (A17); the entry stops reading `projects` (its non-PR fields) and `worktrees`.
- **Planted breaks:** a second writer of the projects record fails the one-writer test; a branch refresh that does not bump `branches(project)` fails; an `is_active` computed on the server fails a source scan; a startup registration that reads `app.tabs` or the App's startup tab fails the flag-on test; a meta-cache interval read from the App fails the held-App-mutex test.
- Size: M.

### P55.2 `prs(remote)` keyed on content, `head_oid` included

- Branch: `epic/p55b-sidebar-prs`. Lanes: DS, US, U, P, D (planned exception, see pair (a)). Model: `opus`. Depends on: P55.1, and the workspace entry holding U when it is ready (pair (a)).
- **Goal:** the sidebar PR lists come from one resource per remote whose version follows its content.
- **Done when:** `prs(remote)` carries the PR rows plus server-built id lists for my PRs, to review, recently merged and saved, with the dismissed filter applied on the server; the writer bumps the version whenever the rows change, any field including `head_oid`, so no length-and-time fingerprint decides it; `patch_pr_head_oid` and the full sweep both write through the owner; the head probe takes its target from the `tabs` resource; `detected_pr_number` (read in `contextIdentity.ts`, `prUrl.ts` and `stores/reviewPageModel.ts`) and the tab staleness the tab strip shows are derived in the frontend from these rows; the PR commands (9) go through `er_send`; the saved-PR cap moves to the server, keeping today's rule: the 51st save drops the oldest, as `projects.rs:190`–`:199` does (`MAX_PR_HISTORY` = 50, `:6`); with the flag on, the `saved-pr` and `dismissed-pr` ops no longer paint and their four commands leave the optimistic path (A8: rule moved to the server); the save-PR median is recorded against the A9 baseline.
- **Planted breaks:** a test drives `patch_pr_head_oid` on a cached row and asserts the version moved and the fetched row carries the new oid; planting a version computed from row counts and fetch times (the `build_projects_cache_key` shape) fails it; a dismiss that does not filter on the server fails the dismissed test; a saved list over 50 fails the cap test; a `saved-pr` or `dismissed-pr` paint with the flag on fails the flag test.
- Size: M.

### P55.3 `inbox` and the inbox optimistic ops

- Branch: `epic/p55c-sidebar-inbox`. Lanes: DS, US, U, P, D (planned exception, see pair (a)). Model: `opus`. Depends on: P55.2, and the workspace entry holding U when it is ready (pair (a)).
- **Goal:** the inbox is a resource, and the two inbox optimistic ops are gone with the flag on (P55.2 removes the other two global ops).
- **Done when:** `inbox` carries the items, the unread count and the last refresh, computed on the server; the inbox commands (9, including `open_inbox_item` and `prefetch_inbox_item`) go through `er_send`; with the flag on, `inbox-read` and `inbox-clear` no longer paint (A8: rule moved to the server for the unread count, deleted for the rest), and `mergeGlobalConfirm` runs flag-off only; the inbox sweep reads its prefs from the settings actor's view instead of `commands::clone_inbox_prefs(&bg_app)` (`main.rs:1609`, `:1642`); the mark-read median is recorded against the A9 baseline; the entry stops reading `inbox_items`, `inbox_unread_count` and `inbox_last_refresh_ms`.
- **Planted breaks:** an unread count computed in TypeScript fails a source scan; a mark-read that does not bump `inbox` fails; an `inbox-read` paint with the flag on fails the flag test; an inbox sweep that reads the App's config fails the held-App-mutex test.
- Size: M.

### P56.1 `diff(tab)`, loading on activation, the warmer and the watcher

- Branch: `epic/p56a-diff-resource`. Lanes: S, D, U, P. Model: `opus`. Depends on: P54.
- **Goal:** the file list, mode, base and staleness of the active tab come from `diff(tab)`, and the background threads that refresh or probe the active tab key it through `tabs`.
- **Done when:** `diff(tab)` carries each file's row and content hash and the diff-level fields, in the file order the App shows, the mtime sort included (`TabState::mtime_cache`, left on the App by the 2a plan); fetching it for a key with no content sends `LoadTab`; the warmer loads unloaded registry tabs at idle through `LoadTab`, scoped by the registry; the refresh, mode, base, filter and sync commands (8) go through `er_send`; the visible-file list uses the pure `visible_files` (`crates/er-engine/src/visible_files.rs:40`, #595); on the flag path the active-branch watcher (`run_active_branch_watcher`, `main.rs:702`) takes its target from `tabs` and the content instead of `desired_local_branch_watch`, replaces `apply_watch_checkout_root` (`:753`) with the content's checkout root, and on a file change sends a new service refresh route with auto-unmark for that key, instead of `refresh_diff_quick_with_unmark` on the App tab (`:783`); it keeps writing `WatchStatusState`, so P53's `session` resource still bumps; the branch-base remote-oid probe (`main.rs:1234`) takes its target from `tabs`; the entry stops reading 17 fields, listed in the issue.
- **Planted breaks:** a followed stub that never loads fails the first-diff test; a warmer that loads the active tab twice fails the warmer test; a filter computed against the App's tab fails the source scan; a stale pill that ignores `preview_head_oid` fails the staleness test; a watcher that refreshes the App tab only leaves the `diff(tab)` version and the `ReviewedSection` revision unmoved after a file write in the watched checkout, and fails; a branch-base probe that reads the App fails the held-App-mutex test.
- **Fallback split (the plan):** P56.1a the resource, the adapter and the routed commands; P56.1b load on activation and the warmer; P56.1c the watcher, its service refresh route and the branch-base probe.
- Size: L.

### P56.2 Content-addressed hunks, previews and file reads

- Branch: `epic/p56b-content-addressed-hunks`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.1.
- **Goal:** hunks are fetched by content hash and cached in the frontend; the server records nothing about what it sent.
- **Done when:** `hunks(hash)` is served per A4; DiffView joins hunks with threads from `annotations(tab)` (adapter-fed until P58.1); the frontend cache is an LRU bounded by bytes; previews, compaction expansion and `list_diff_paths` (5 commands) run on the service; with the flag on, no code path calls `record_sent_file` or reads `hunks_omitted`.
- **Planted breaks:** a hash that includes threads makes a comment re-send the file and fails the comment test; an unbounded cache fails the byte-bound test; a fetch of an unparsed file that parses on the reader thread instead of through `ParseFiles` fails the one-parse-path test; a flag-on call to `record_sent_file` fails the source scan.
- **Fallback split:** P56.2a hunks and the cache; P56.2b previews and file reads.
- Size: L.

### P56.3 History, Conflicts and Hidden in the service

- Branch: `epic/p56c-history-modes`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.2.
- **Goal:** the three modes that load on the App with the flag on load in the Workspace.
- **Done when:** the History commit list and the selected commit's files are a section of `TabContent` and part of `diff(tab)`; Conflicts and Hidden load by the engine steps the App runs, extracted into the always-compiled half as 2a.1 did; `select_commit` and `exit_history` go through `er_send`; a History file click no longer reaches the App; `LoadTab` in History, Conflicts or Hidden succeeds, and a recorded mode takes the new view's context and AI bucket (#596 review); the preview key clears `history_commit` outside History on both paths, so the App's and the service's preview keys agree (#595 note 1; `er-service/src/content.rs:77`); the parity test covers the three modes and the preview stamps; the entry stops reading `commits` and `selected_commit_sha`.
- **Planted breaks:** skipping the commit list load fails the History parity row; a click routed to `history_select_file` with the flag on fails the source scan; a Conflicts load that reads the App fails the actor-thread scan; a `LoadTab` in History that fails, or a recorded mode that keeps the old AI bucket, fails the mode-load test; a preview key that keeps `history_commit` outside History fails the preview-stamp parity row.
- **Fallback split:** P56.3a the engine extraction (flag-off byte-identical); P56.3b the service and the routes.
- Size: L.

### P57 `reviewed(tab)` and frontend navigation

- Branch: `epic/p57-reviewed-navigation`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.2.
- **Goal:** reviewed marks are a resource, reviewed writes name their tab, and navigation runs in the frontend.
- **Done when:** `reviewed(tab)` carries the marks and the counts; the reviewed commands carry the `TabKey` the page rendered and are refused when it is not the content's key; the checkmark is pending until ack (A8); selection, cursor and `next_file`, `prev_file`, `next_hunk`, `prev_hunk` and `jump_to_unreviewed` run in desktop-ui over the two resources (A10), checked by the shared fixture against the Rust functions; with the flag on, `bulk-reviewed` no longer paints and `bulk_review_pillar` and `unbulk_review_pillar` leave the optimistic path (A8: deleted; the `reviewed(tab)` push paints it); the mark-reviewed and next-file medians are recorded against the A9 baseline; the entry stops reading `reviewed_count`, `selected_file` and `current_hunk` and 12 commands.
- **Planted breaks:** a reviewed write without the key lands on the registry's other tab and fails the lag test; `reviewed_count` computed in TypeScript fails the source scan; a TypeScript `jump_to_unreviewed` that skips the last file fails the shared fixture; a `bulk-reviewed` paint with the flag on fails the flag test.
- **Fallback split:** P57.1 reviewed; P57.2 navigation.
- Size: L.

### P58.1 `annotations(tab)` and `ui_annotations(tab)`

- Branch: `epic/p58a-annotations-read`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.2.
- **Goal:** findings, threads, tour, checklist, triage, diagrams and the other bucket are read from resources.
- **Done when:** `annotations(tab)` is built from the AI section with thread ordering and counts on the server; `ui_annotations(tab)` is served by the desktop and bumped by its writes; the sidecar reload watches the comments directory as well as `er_dir`, driven by P52.1's backend tick (A17) with no frontend call; `load_tab` reads the sidecar stamps before the AI read, so an outside write between the two is not lost (#596 review); a spawn failure in `SidecarReloads::start` (`er-service/src/sidecar_reload.rs:40`) removes the key from `running` (`:54`), so the next reload runs (#596 review); `annotations(tab)` carries the triage review update as a protocol type, so `TriageReviewUpdateSnapshot` (`triage_update.rs:13`) is flag-off only; the four read commands go through `er_fetch`; the entry stops reading `ai`, `tour`, `other_bucket` and `ui_annotations`.
- **Planted breaks:** an outside write to `questions.json` moves the `annotations(tab)` version with no frontend call, and the reload test fails when the comments directory is dropped from the watch or the tick is disabled; a stamp read after the AI read loses a write landed between them and fails the ordering test; a spawn failure that leaves the key in `running` makes the next reload answer false and fails; thread ordering done in TypeScript fails the source scan; an arena tick that bumps `annotations(tab)` fails the precision test.
- **Fallback split:** P58.1a the builders and the reload; P58.1b the desktop-ui port.
- Size: L.

### P58.2 Annotation writes and pending until ack

- Branch: `epic/p58b-annotation-writes`. Lanes: D, U, P. Model: `opus`. Depends on: P58.1, P55.3 (both edit `optimisticLocal.ts`).
- **Goal:** the 27 annotation write commands go through `er_send` and the thread paints shrink to pending until ack.
- **Done when:** each write maps to the service command P41 added, by `TabKey`; the 17 remaining `OptimisticOp` kinds are classified by name (A8). Pending until ack, with the `client_id`, the confirming fetch landing on the same row (ADR 0017): `add-thread`, `reply`, `promote`, `promote-finding` and `reply-finding` (its `existing` and `new` targets; the `ai` target paints nothing). Deleted with the flag on, the `annotations(tab)` or `ui_annotations(tab)` push painting the result: `resolve`, `delete-root`, `delete-reply`, `edit`, `dismiss-finding`, `resolve-finding`, `add-annotation`, `delete-annotation`, `checklist-toggle`, `remove-finding-thread`, `delete-finding-response` and `edit-finding-response`. Thread ordering, the one rule these paints applied, moved to the server in P58.1. The add-question median is recorded against the A9 baseline, and the deleted paints are timed click-to-paint with `profileLog`, numbers in the PR.
- **Planted breaks:** dropping `client_id` makes the confirmed row get a fresh id and fails the confirm test; a `resolve` still painted with the flag on fails the flag test; a `reply-finding` with a `new` target that does not paint pending fails the pending test.
- **Fallback split:** P58.2a threads, comments and promote; P58.2b findings, checklist, artifacts and browser annotations.
- Size: L.

### P59 `pr(tab)`, `pr_status`, the stack and the GitHub senders

- Branch: `epic/p59-pr-resources`. Lanes: S, D, DS, U, P. Model: `opus`. Depends on: P58.1, P55.3 (DS is free once the sidebar track ends).
- **Goal:** the PR card, merge box, GitHub status and stack read resources, and no GitHub command picks its tab from the App.
- **Done when:** `pr(tab)` is built from the PR section; `pr_status(remote, number)` from the status cache through the desktop owner; the stack is part of `pr(tab)`: `PrSection` gains it, and a service command for the stack refresh runs `gh_stack::load_from` off the actor thread, as `refresh_stack_impl` (`commands.rs:3295`) runs it off the App lock, so the actor stays its one writer and a stack change moves the PR section's revision (decided: the stack is per tab, refreshed with the PR and read only by the PR card, so its own resource would add a fetch to every card render); `active_tab_command`, `send_for_active_tab`, the comment tick, `abort_wrong_view` and the document preview check take the tab from the `tabs` resource and its content; the GitHub status loop (`main.rs:1386`) takes its PR identity from `tabs` and `prs(remote)` instead of `active_pr_identity(g.tab(), …)` (`:1395`); the 13 GitHub command functions (12 names; `submit_github_pr_decision` has two bodies) go through `er_send`; the entry stops reading `pr`, `github` and `stack`.
- **Planted breaks:** a GitHub post that reads the App's active tab fails the held-App-mutex test; a status loop that reads the App tab fails the same test; a status refresh that does not bump `pr_status` fails; a stack refresh that does not move the PR section's revision fails; a merge pinned to a stale head fails the head test.
- **Fallback split:** P59.1 `pr(tab)`, `pr_status` and the stack; P59.2 the GitHub senders and commands.
- Size: L.

### P60.1 Arena and review flows on `jobs`

- Branch: `epic/p60a-jobs-arena`. Lanes: S, D, U, P. Model: `opus`. Depends on: P54. Runs beside P59 (pair (b)).
- **Goal:** the arena page and the flow strip read `jobs`, and arena and flow commands stop resolving their tab on the App.
- **Done when:** `jobs` carries the live arena runs, the review flows and their progress across actors; arena `resolve` and flow `key` take the tab from the `tabs` resource; the 20 arena and flow commands go through `er_send` and their reads through `er_fetch`, which retires the three `ArenaRunSnapshotWire` commands (`flow_get_run`, `arena_get`, `arena_override`); an arena run and a review flow on a remote PR tab key through `tabs`, covering #577's surviving plants 8 and 9; `App.svelte:293`'s `er://revision` listener is replaced by the `jobs` resource; the entry stops reading `review_flow`, `active_arena_run`, `arena_runs` and `background_arena_runs`.
- **Planted breaks:** an arena start that resolves the App's tab fails the held-App-mutex test; #577's plants 8 and 9 (re-applied) fail the new remote-PR-tab arena and flow tests; a progress tick that does not bump `jobs` fails; the Deep cancel and orphan test from P28b/P32b is reused unchanged.
- **Fallback split:** P60.1a the `jobs` aggregate and the arena; P60.1b review flows and the `App.svelte` listener.
- Size: L.

### P60.2 AI runs, triage and task logs on `jobs`

- Branch: `epic/p60b-jobs-runs`. Lanes: S, D, DS, U, P. Model: `opus`. Depends on: P60.1, P55.3 (DS is free once the sidebar track ends).
- **Goal:** card AI, review runs, triage and the background task panel read `jobs`.
- **Done when:** the 19 run, triage and log commands go through `er_send`/`er_fetch`; triage spinners read `jobs` (owner answer); auto-triage writes its queue through the desktop owner; the tick's four agent and task duties (`drain_agent_log`, `check_commands`, `poll_background_tasks`, `process_ai_task_inbox`) move off the App with the runs they drive, so no tick call names the App after this entry (A17); the entry stops reading `agent_commands`, `agent_log`, `background_tasks` and `bg_loading`.
- **Planted breaks:** a triage queued without a `jobs` bump leaves the spinner off and fails; a task log fetched from the App fails the source scan; a tick that still calls an App duty fails the same scan; a cancel that does not reach the runner fails the reused `CancelJob` assertion (P43).
- **Fallback split:** P60.2a AI runs and triage on `jobs`; P60.2b agent commands, logs, background tasks and the tick's App duties.
- Size: L.

### P61 Window-mechanics commands typed and moved out of `commands.rs`

- Branch: `epic/p61-window-commands`. Lanes: D, U, P, C. Model: `opus`. Depends on: P55.3, P57 (desktop-ui owns the selection the open commands now take as parameters), P60.2 (it holds D and U, so it runs after both tracks).
- **Goal:** the 27 commands that stay direct return `Result<T, ErError>` with protocol types, and none of them lives in a file P62.2 deletes.
- **Done when:** the browser webview (9), terminal (4), OS launch and reveal (6), export (3), `start_window_drag`, `detect_dev_url`, `line_commits`, `fetch_github_image` and `dev_log_filter` commands are typed; the ones in `commands.rs` (16), `commands/line_commits.rs` and `arena_commands.rs` (`dev_log_filter`) move into a new `crates/er-desktop/src/direct/` module, one file per group (window, OS launch and reveal, terminal, export, line commits, dev log), so P62.2 can delete `commands.rs`, the `commands/` directory and `arena_commands.rs` whole; `browser_webview.rs` and `github_image.rs` stay where they are; the commands that read the App take what they need as parameters: `open_in_editor`, `open_in_vscode` and `open_source` take the path and line from desktop-ui, which owns the selection since P57 (today they read `tab.selected_diff_file()` and `current_hunk`, `commands.rs:1666`–`:1681`), `reveal_er_folder` takes the `TabKey` and reads `er_dir` from the content (today `app.tab().er_dir()`, `:2436`), and the export commands take the `TabKey` and render from the tab's published content instead of `render_markdown(app.tab(), …)` (`:10551`), which moves `export.rs` off the `ui`-gated `er_engine::export` before P62.2 needs it; `export_to_agent` stops returning `AppSnapshot` and its run shows in `jobs` (P60.2); the names among them that fail the rule leave the allow-list; their callers use the generated types.
- **Planted breaks:** restoring one to `Result<_, String>` fails `er::command_shape`; leaving a typed name on the allow-list fails as stale (P51), and so does re-adding one; an `open_source` that reads the App's selection fails the held-App-mutex test; an export rendered from `TabState` fails the export parity test against the flag-off output.
- **Fallback split:** P61.1 the move and the typing; P61.2 the App-free parameters and the export renderer.
- Size: L.

### P62.1 Delete the old store layer

- Branch: `epic/p62a-delete-old-store-layer`. Lanes: U, P, C, D. Model: `opus`. Depends on: P53, P54, P55.1, P55.2, P55.3, P56.1, P56.2, P56.3, P57, P58.1, P58.2, P59, P60.1, P60.2, P61.
- **Goal:** desktop-ui runs only on resources.
- **Done when:** the adapters lose their flag-off branch; the `snapshot` store, `poll` handling, `optimisticLocal.ts`'s remainder, `snapshotDelta.ts`, `snapshotChrome.ts`, `tabSnapshotCache.ts` and `v1ServiceFlag.ts` are deleted; the hand-written `types.ts` is replaced by a generated barrel that `protocol-check` covers, and frontend-only types move to their modules; `scripts/tauri-dev.sh` drops the flag; er-desktop makes `v1-service` a default feature, so a backend built without it cannot pair with the new frontend. **Test change, up front:** P38's default-build assertion in the bin target (`!cfg!(feature = "v1-service")`, `crates/er-desktop/src/main.rs:2347`) flips to assert the feature is on, and from here `test-desktop` runs the flag-on path, which makes `test-v1` redundant for er-desktop; the flag-off backend tests compile out with their `cfg`.
- **Planted breaks:** a hand edit of `types.ts` fails `protocol-check`; a component reading `app.snapshot` fails the source scan; `check-ui` fails on a removed field still referenced; building er-desktop with `--no-default-features` fails the flipped assertion.
- Size: M (deletions do not count).

### P62.2 Delete the old desktop path and the desktop flag

- Branch: `epic/p62b-delete-old-desktop-path`. Lanes: D, DS, C, P, U. Model: `opus`. Depends on: P62.1.
- **Goal:** the phase exit.
- **Done when:** `snapshot.rs` and `commands.rs` are deleted, with the `commands/` directory (`content_routes.rs`; `line_commits.rs` left in P61), the App-backed desktop route, `get_snapshot`, `poll`, `v1_service_enabled`, `run_revision_watcher`, the flag-off bodies of the three resource commands (A2) and the follower; the old commands outside `commands.rs` go too, each replaced by `er_send`/`er_fetch` in the entry named: `arena_commands.rs` whole (19 commands, `:135`–`:667` at `949eb993d`, P60.1; `dev_log_filter` left in P61), `config_commands.rs` whole (11, `:32`–`:128`, P53), `risk_commands.rs` whole (2, `:17` and `:29`, P53) and `gh_pr_actions.rs`'s `run_github_pr_action` (`:64`, P59); any helper an `er_send` mapping still calls moves into the module that calls it in that entry; the two `*Snapshot` types outside `snapshot.rs` go: `SettingsSnapshot` (`settings.rs:12`) with `GetConfigHubResponse` (`:20`), replaced by P53's `settings` resource, and `TriageReviewUpdateSnapshot` (`triage_update.rs:13`), replaced in P58.1; er-desktop has no `er_engine::app` reference and depends on er-engine with `default-features = false, features = ["watch"]` (the watcher, `main.rs:707`; P61 already moved `export.rs` off the `ui`-gated `er_engine::export`), and its dev-dependency becomes `default-features = false, features = ["test-support"]`, so a test build does not turn `ui` back on; a new `check-desktop` recipe (`cargo check -p er-desktop`, in the style of `build-service-headless`) joins `just ci` and a CI job with `tauri-linux-deps`, because resolver 2 unifies er-engine's features with er-tui in every workspace build, so `clippy --workspace` and `test-desktop` alone would still compile `ui`; the #578 follower notes deferred here (the gating scan's `cfg(any(…, test))`, the remote-PR row check that cannot fail, m02) close with the follower; the `v1-service` feature leaves er-desktop and stays on er-tui (A12), and `test-v1`, CI and the lint gate's second pass keep only er-tui; the `er::command_shape` allow-list is empty and deleted, and no command returns a `*Snapshot` type or `Result<_, String>`; ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034 are marked "Superseded by" the protocol ADR, and docs-check fails when a superseded mark and the superseding ADR's list disagree; `CLAUDE.md`'s push-driven line, `crates/er-desktop/agent.md`, `desktop-ui/agent.md` and `desktop-ui/src/lib/stores/agent.md` describe resources; `app-commands.toml` and `ipcAllowlist.test.ts` list the surviving commands only.
- **Planted breaks:** a command returning `Result<AppSnapshot, String>` fails `er::command_shape` with no allow-list to hide in; a `use er_engine::app::App` in er-desktop fails `check-desktop` locally and in CI; deleting 0012 from the protocol ADR's supersedes line fails docs-check; a compile error behind `cfg(feature = "v1-service")` in er-tui still fails CI on a throwaway draft PR (#470's shape); the flipped `cfg!` assertion goes with the feature, and a test that reads er-desktop's manifest fails when a `v1-service` feature is re-added.
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
| **U** desktop-ui | `desktop-ui/src/**` except the US files and `desktop-ui/src/lib/protocol/**` |
| **US** sidebar UI | `LeftSidebar.svelte`, `InboxView.svelte`, `InboxPanel.svelte`, `sidebarModel.ts`, `sidebarPrOpen.ts`, `inbox*.ts`, `projectOrder.ts` and the sidebar adapter stores |

- **Chain:** P50 → P51 → P52.1 → P52.2, then two tracks:
  - workspace: P53 → P54 → P56.1 → P56.2 → { P56.3, P57, P58.1 } in that
    order → P58.2 → { P59 ∥ P60.1 } → P60.2;
  - sidebar: P55.1 → P55.2 → P55.3.
  - Both tracks join at P61 → P62.1 → P62.2. P61 holds D and U, which every
    workspace entry holds, so it runs after both tracks.
- **Pair (a), the two tracks, a planned exception.** The sidebar entries
  hold D for these edits only, each conflict-by-addition or in a block of
  `main.rs`'s setup function that no workspace entry edits:
  - every sidebar entry: a dispatch arm per sidebar key in the resource host;
  - P55.1: the startup registration block (`main.rs:997`–`:1003`), the
    meta-cache interval (`:1708`) and the tick's `kick_worktrees_warm`
    target (A17). The registration block ends five lines above P56.1's
    warmer scope (`:1008`), so whichever of P55.1 and P56.1 merges second
    rebases there by hand;
  - P55.2: the PR head probe thread (`:1316`);
  - P55.3: the inbox sweep's prefs (`:1609`, `:1642`);
  - the workspace track's `main.rs` blocks are P54's `launch_root`
    (`:1059`), P56.1's watcher (`:702`), branch-base probe (`:1234`) and
    warmer (`:1724`), and P59's GitHub status loop (`:1386`);
  - nothing in `snapshot.rs`, because the owner wraps the existing metadata
    and PR caches and calls `refresh_meta_cache` unchanged.

  P (new `ResourceKey` variants, generated files) is shared the same way;
  generated files are regenerated. P55.2 and P55.3 also hold U: P55.2 for
  the `detected_pr_number` and tab-staleness derivations
  (`contextIdentity.ts`, `prUrl.ts`, `stores/reviewPageModel.ts`, the tabs
  adapter) and its `optimisticLocal.ts` edits, P55.3 for its
  `optimisticLocal.ts` and `app.svelte.ts` edits. Each waits for the
  workspace entry holding U to merge and takes U before the next workspace
  entry starts, and P58.2 waits for P55.3. The workspace track takes DS only
  in P59 and P60.2, both after P55.3.
- **Pair (b), P59 beside P60.1.** P59 edits the GitHub senders and the PR
  card; P60.1 edits `arena_service.rs`, `flow_service.rs`,
  `arena_commands.rs` and `desktop-ui/src/lib/stores/arena.svelte.ts`. Shared files:
  `ResourceKey`, the host dispatch, the wire command enum. Additions only.
- PLAN's three-lane ceiling counts tracks here: two tracks at most, plus a
  C-only entry when one exists.

## Risks

1. **Latency the optimistic layer hid.** Each entry that deletes the paint
   of one of A9's hot actions records that action's old and new medians in
   its PR; an overrun keeps that one paint as pending until ack. The other
   deleted paints (most of P58.2's) are timed click-to-paint with
   `profileLog` and the numbers go in the PR, with no budget.
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

- P52.2 adds the protocol ADR and the **Resource** term in
  `CONTEXT.md`.
- P62.2 marks ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034 superseded by
  the protocol ADR, adds the two-way supersession check to docs-check, and rewrites the
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
  Since the pin, #578 has merged into `release/v1.0.0` at `5942bc925`, and
  P47.2c.2's local head is `2f709c637` (no PR yet). Line numbers are re-found
  at those heads in gate step 3, not before.
- Phase 5b and phase 6 are out of scope: the TUI keeps the `App`,
  `navigation.rs`'s `App` methods and the `TabState` view fields (D20);
  inbox, projects, MCP and the CLI move in phase 6.
