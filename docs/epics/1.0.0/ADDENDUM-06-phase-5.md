# Addendum 06: phase 5, desktop protocol

Drafted for the phase-5 gate (#398), following [PLAN.md "Re-planning at a
phase gate"](./PLAN.md#re-planning-at-a-phase-gate). Where this file disagrees
with PLAN.md or an issue body, this file wins once committed;
[ADDENDUM-01](./ADDENDUM-01-decisions.md),
[ADDENDUM-02](./ADDENDUM-02-issue-decisions.md) (D11–D22) and
[ADDENDUM-05](./ADDENDUM-05-phase-4.md) still bind.

Every claim and citation below was checked against `release/v1.0.0` at
`1f7527a1f` (2026-11-17), the merge of #600, which merged `main` into the
release branch (D2). That ref holds every phase-4 PR: #578 (P47.2b.2, merge
`5942bc925`) and #599 (P47.2c.2, merge `e2fd75582`), so no citation here
points at an unmerged head. The round-1 to round-3 drafts were pinned at
`949eb993d`; each site was re-found at `1f7527a1f` by symbol. Symbols are
the anchor; line numbers are a convenience.

## Claims re-checked against the tree

Method: `git grep` and `git show` at `1f7527a1f`; command signatures were
parsed from the line after each `#[tauri::command]` attribute to the opening
brace.

- **Command surface.** er-desktop has 205 `#[tauri::command]` functions with
  200 distinct names: `new_tab`, `close_tab`, `select_tab`, `reorder_tabs` and
  `submit_github_pr_decision` each have a flag-on and a flag-off body under
  `cfg`. All 200 names are registered in the one `generate_handler!` list
  (`main.rs:2029`–`:2230`). By file: `commands.rs` 159, `arena_commands.rs` 20,
  `config_commands.rs` 11, `browser_webview.rs` 9, `risk_commands.rs` 2, and
  one each in `crates/er-desktop/src/commands/line_commits.rs`, `gh_pr_actions.rs`, `github_image.rs`
  and `main.rs` (`v1_service_enabled` `:890`). So 46 commands sit outside
  `commands.rs`, and the exit rule covers all of them.
- **Return types.** 129 commands return a type named `*Snapshot`
  (`AppSnapshot` 127, `FileSnapshot` 1, `AgentLogSnapshot` 1); three more
  return `ArenaRunSnapshotWire`, whose name contains `Snapshot` without ending
  in it. 201 return `Result<_, String>`. Four return neither:
  `dev_log_filter` (`arena_commands.rs:700`), `list_ai_experts`
  (`commands.rs:4554`), `list_ai_reviewers` (`:4559`) and
  `v1_service_enabled`.
- **Snapshot types.** er-desktop declares 42 `pub struct *Snapshot` types:
  40 in `snapshot.rs`, `SettingsSnapshot` (`settings.rs:12`) and
  `TriageReviewUpdateSnapshot` (`triage_update.rs:13`). `AppSnapshot`
  (`snapshot.rs:458`) has 61 top-level fields. `snapshot.rs` is 8,344 lines,
  `commands.rs` 16,558, `main.rs` 2,788.
- **The old protocol's moving parts.** `poll` (`commands.rs:11981`,
  `poll_impl` `:11986`), `compute_chrome_revision` (`:12177`) and
  `compute_content_revision` (`:12244`), `snap_from_command` (`:359`) and
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
- **The flag.** 302 lines in `er-desktop/src` name `feature = "v1-service"`, 199 of
  them in `commands.rs`. `just test-v1` runs
  `er-desktop --lib` and `er-tui` with the feature; CI's `Test (v1-service)`
  job (`ci.yml:83`) runs the same two. desktop-ui checks the build value in
  `v1ServiceFlag.ts`.
- **The engine split.** Only `app` and `export` are `ui`-gated in
  `crates/er-engine/src/lib.rs`; `arena` and `navigation` are always compiled.
  er-desktop depends on er-engine with default features and names
  `er_engine::app::` on 160 lines (98 in `commands.rs`; #578 and #599 added
  one each, in `tabs/follow.rs` and `commands/tests/tab_key_tests.rs`). Its `export.rs` calls
  the `ui`-gated `er_engine::export`.
- **No codegen exists.** No crate depends on `ts-rs` or `specta`.
- **The sidebar's PR fingerprint ignores `head_oid`.** `build_projects`
  (`snapshot.rs:4554`) memoizes the project rows behind `ProjectsCacheKey`
  (`:4583`); `build_projects_cache_key` (`:4594`) hashes each remote's PR count
  and last fetch time. The 30 s head probe (the thread at `main.rs:1357`) calls
  `patch_pr_head_oid` (`pr_cache.rs:114`, call `main.rs:1403`), which changes
  one row's `head_oid` in place and touches neither, so the sidebar keeps the
  old oid until the next full sweep. The coordinator's "about line 4484" was a
  `main` line number; at this ref it is `:4594`. The probe also picks its
  target from the App's active tab (`pr_head_probe_target`, `main.rs:656`).
- **Sidebar sources are desktop-side** (D19): projects (`projects.rs`), the
  branch metadata cache (`ProjectMeta` `snapshot.rs:2377`,
  `refresh_meta_cache` `:2388`), the PR list cache (`pr_cache.rs`), GitHub
  status (`gh_status_cache.rs`), inbox (`InboxState` `inbox.rs:167`, read by
  `snapshot_inbox` `snapshot.rs:943`) and auto-triage (`auto_triage.rs`).
- **The read seam exists.** `SnapshotRead` (`snapshot.rs:3101`), `AppRead`
  (`:3144`), `build_snapshot_from` (`:3276`) and `ViewRead`
  (`snapshot_view.rs:27`) came with P47.1a; `DesktopViewState`
  (`view_state.rs:20`) holds panels, browser state and `TabNavState` (`:75`)
  per `TabKey`.
- **Latency harnesses.** Two ignored harnesses exist:
  `the_actor_hop_stays_within_the_latency_budget`
  (`er-service/src/tests.rs:209`) and
  `the_content_routes_stay_within_the_latency_budget`
  (`content_tests.rs:464`).
- **`main` and release agree.** #600 merged `main` (the CI commits of
  #575) into release; `git log origin/release/v1.0.0..origin/main` is empty
  at `1f7527a1f`.

## Phase-4 changes as landed

P38 through P47.2 are merged: P47.2's layers 2a.1–2a.3 (#570, #572–#574,
#576), 2b (#577, #578), 2c.1 (#595, #596, #597) and 2c.2 (#599), and #538 is
closed with the phase-4 exit checklist. What phase 5 builds on:

- **The service owns tab state and routes** (owner, 2026-11-16, P47.2c slim).
  The open, diff, file, reviewed and annotation routes run through the
  Workspace on the flag path; the flag-on snapshot still reads the App, through
  `AppRead`. Phase 5 replaces that rendering.
- **Tab content is published** as `Arc` sections with their own revisions (ADR
  0071), including the PR section, preview stamps, every diff mode recorded,
  and `ReloadSidecars` from the poll tick (#596).
- **One actor key and one tab key.** `workspace_key` replaced `service_key`
  (#577); #578 made the registry issue every key and the App follow it by
  key.
- **App-scoped records have one service owner** (ADR 0070): `AppRegistry` for
  tabs and the open caches, the settings actor for config (ADR 0069). The
  sidebar sources stay in the desktop under D19.

### Phase-4 leftovers and where each goes

These come from #578's follow-ups and review, #596, and the body of #599,
which #538's closing body copies ("Left on the App with the flag on" and
"Follow-ups from the merged PRs"). Every site is cited at `1f7527a1f`.

- Followed and restored stubs never get their first diff with the flag on
  (the content loads on a tab's first route, `active_content`) →
  **P56.1** (fetching `diff(tab)` for a key with no content sends `LoadTab`).
- The background tab warmer (`main.rs:1765`) is off with the flag on →
  **P56.1** (warm unloaded registry tabs through `LoadTab` at idle).
- `DropTab` publishes a revision for a key that never loaded → **P54** (a
  drop of a never-loaded key moves no resource version).
- An all-broken restore keeps one broken tab → **P54**; what it shows instead
  is owner question (4) below.
- A close that lands before the actor starts sends no `DropTab`
  (`send_if_started`, `service.rs:184`) → **P54** (a `LoadTab` for a key
  the registry no longer lists is refused).
- `WorkspaceState::closed` grows for the life of the actor
  (`workspace.rs:153`) → **P54** (bounded, with a test).
- `same_pr` requires the same tab kind (`content_routes.rs:122`) → **P54**,
  as A15 below.
- `launch_root` (`main.rs:1088`) and the warmer scope (`:1037`) read the
  startup tab on the flag path → **P54** and **P56.1**. The startup
  registration block (`main.rs:1026`–`:1032`: `sync_projects_from_tabs(&app.tabs)`,
  `projects.rs:472`, then `auto_register` on `app.tab().repo_root`) and the
  flag-on block's second `sync_projects_from_tabs(&app.tabs)` after
  `tabs::restore` (`main.rs:1132`, inside `:1127`–`:1136`) → **P55.1**.
- `toggle_reviewed`, `mark_reviewed` and `unmark_reviewed` (`write_reviewed`,
  `content_routes.rs:475`; #599 moved `unmark_reviewed` onto it) send
  without the painted tab's key, so during the follower's lag they write to
  the registry's active tab → **P57** (decision A, below): the command
  carries the `TabKey` the page rendered, and a mismatch is refused. P62.2
  deletes the follower, which closes the class.
- The sidecar reload watches `er_dir` only (#596; `readers`,
  `er-service/src/sidecar_reload.rs:115`) → **P58.1**.
- A write the workspace itself made is read back once by the next tick,
  because the sidecar stamp is not advanced on its own writes (#596;
  `TabContent`'s write path keeps the old stamp, `content.rs:154`–`:155`)
  → **P58.1**: with P52.1's tick reloading every loaded key, each service
  annotation write would otherwise cost a second `annotations(tab)` version
  and a refetch.
- Only the frontend's `poll` drives `poll_impl` (`commands.rs:11986`, its one
  caller `app.svelte.ts:589`), and its first pass is the only trigger for
  `content_routes::reload_sidecars` (`commands/content_routes.rs:366`, active
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
    the remote-PR row check "replaced tab's content stayed" in
    `every_kind_keeps_one_key_through_open_duplicate_and_replace`
    (`commands/tests/tab_key_tests.rs:158`) cannot fail; m02 (stubs built
    outside the App lock) is untested → **deferred to P62.2**: #578 records
    all three as closing when the follower is deleted, and P62.2 deletes it;
    a fix before then would test code that P62.2 removes.
- Cmd-T on a remote PR tab opens a rootless Working tab (`service_new_tab`,
  predates phase 4) → **P54**.
- **Left on the App with the flag on.** #538's list, reconciled entry by
  entry, plus what a search over all of `crates/er-desktop/src`
  (`er_engine::app`, `&App`, `.tab()`, `state.app`, `app.tabs`, `app_arc`)
  finds outside the command bodies #599 read:
  - rendering: `get_snapshot` and `poll` build every `AppSnapshot` section
    through `AppRead`, and every command's returned snapshot reads the App
    (#538's "reply only" bucket: the tab commands, the inbox commands through
    `mutate_inbox`, the projects chrome through `confirm_projects_chrome`)
    → each entry's `er_send` mapping answers an `Ack` instead; P62.2
    deletes `get_snapshot` and `poll`;
  - History, Conflicts and Hidden load on the App (`commands.rs:1345`), and
    History file clicks reach `history_select_file` (`select_file_routed`,
    `commands.rs:1111`) → **P56.3**;
  - compaction (`toggle_compacted`, `commands.rs:1304`): `ParseFiles {
    expand: true }` expands, but the protocol has no re-compact
    (`er-service/src/protocol.rs:244`–`:249`) → **P56.2**, which adds it;
  - file and image previews, `list_diff_paths` → **P56.2**;
    `read_review_json` and `list_review_revisions` → **P58.1**;
  - navigation (`next_file`, `prev_file`, `next_hunk`, `prev_hunk`,
    `jump_to_unreviewed`) → **P57**; view state on `TabState`
    (`set_filter`, `clear_filter`, `change_base`) → **P56.1**, and
    `open_commit_composer` → **P57**;
  - the document preview check of `request_file_preview`
    (`commands.rs:1129`), which compares the App tab's
    `preview_context_key` before and after the read, the "Document preview
    context changed" guard (`:1138`–`:1139`, `:1151`–`:1155`) → **P56.2**,
    which ports that command;
  - the annotation writes' view checks. `route_annotation` (`commands.rs:6873`)
    takes the App lock to refuse a write whose painted view differs
    (`abort_wrong_view`, `:6883`), which also guards the follower lag, for
    the 14 commands #538 lists under it; `service_add_document_thread`
    (`:2843`) reads the App tab's preview twice (`:2861`–`:2870`,
    `:2875`–`:2883`) → **P58.2**: each write carries the `TabKey` and view
    the page painted, and the document thread compares the content's preview
    key. `abort_wrong_view` (`:343`) has 23 call sites; with the flag on it
    runs through `route_annotation`, through the finding, checklist and UI
    annotation edits below (all P58.2) and through `reply_to_finding_impl`
    (P60.2). Each of those entries replaces the check with the keyed refusal;
    P62.2 deletes the function with the flag-off bodies;
  - finding and checklist edits that check the painted view on the App
    (`dismiss_finding`, `resolve_finding`, `unresolve_finding`,
    `remove_finding_thread`, `update_finding_response`,
    `delete_finding_response`, `toggle_checklist_item`,
    `delete_review_artifact`, `delete_diagram`) and the UI annotation writes
    → **P58.2**; `list_ui_annotations` → **P58.1**;
  - `reply_to_finding` with `ai_assist` (`commands.rs:10389`): with the
    flag on, `FindingReplyRoute::FlagOff` (`:10411`–`:10413`) sends it to
    `reply_to_finding_impl` (`:10437`), which locks the App (`:10445`), reads
    `app.tab().ai.review` (`:10458`), writes the stub thread on the App
    (`:10492`) and calls `ask_ai_impl` (`:10522`) → **P60.2**; P58.2 maps
    `ai_assist = false` only;
  - the session AI pick: `App::current_ai_provider`, `current_ai_model` and
    `current_ai_effort`. Writers with the flag on: the flag path's
    `ai_selection_service::snapshot` (`ai_selection_service.rs:29`, App lock
    `:30`, write `:32`–`:34`) and the `persist: false` arms of
    `ai_selection` (`commands.rs:5179`, write `:5223`–`:5225`) and
    `ai_effort` (`:5243`, write `:5289`), which run with the flag on too.
    Readers: `list_ai_providers` (`:5152`–`:5153`) and the session row
    (`snapshot.rs:3230`) → **P53**; `arena_service.rs:48` and
    `flow_service.rs:25` → **P60.1**; `ask_ai_impl` (`commands.rs:5834`,
    `:5835`, `:5880`, `:5881`, `:5907`) and `ask_ai_for_finding`
    (`:6112`, `:6113`, `:6151`) → **P60.2**. A18 below names the new source;
  - AI runs that read their context from the App tab (`run_ai_review` and the
    rest of #538's list, `ask_ai` and `elaborate_with_ai` through
    `ask_ai_impl`, `validate_with_ai`, `send_note_to_agent`,
    `cancel_queued_review`, `cancel_running_review`,
    `get_background_task_log`) → **P60.2**; `export_to_agent` → **P61**;
  - GitHub senders keyed off the App tab: `active_tab_command`
    (`commands.rs:6895`), `send_for_active_tab` (`:6904`),
    `run_github_pr_action_blocking` with `active_github_key`
    (`gh_pr_actions.rs:79`–`:80`, and the App snapshot at `:101`–`:102`),
    `refresh_stack` through `refresh_stack_impl`, and the flag path's 45 s
    comment tick `spawn_service_comment_sync` (`main.rs:812`, started at
    `:1483`), which picks its PR with `app.try_lock()` and
    `active_pr_identity(guard.tab(), &pr_cache)` (`:823`–`:826`) → **P59**;
  - GitHub and projects commands: `refresh_github_status` and
    `submit_github_pr_decision` → **P59**; `sync_pr` and `sync_branch` →
    **P56.1**; `refresh_pr_list`, `refresh_project_pr_list`, `track_pr`
    and `untrack_pr` → **P55.2**; `add_tracked_branch`,
    `remove_tracked_branch`, `delete_project`, `reorder_projects` and
    `patch_project_review_settings` → **P55.1**; `refresh_notifications`
    → **P55.3**;
  - the browser helpers `tab_browser` (`browser_webview.rs:230`),
    `sync_tab_browser_chrome` (`:244`) and `on_tab_selected` (`:343`) take
    `engine: &App` and find the tab's key by App index. Their flag-on callers
    are `update_tab_browser` (`commands.rs:11563`: `app.active_tab`
    `:11576`, `set_active_tab_browser(&app, …)` `:11587`, the helpers at
    `:11589` and `:11597`) and `cycle_tab_browser_layout` (`:11626`:
    `app.active_tab` `:11632`, `app.tab().key()` `:11635`, the helper at
    `:11638`), which compile in both builds. The calls in `close_tab`
    (`:10910`) and `select_tab` (`:10943`) are flag-off twins
    (`#[cfg(not(feature = "v1-service"))]` at `:10897` and `:10922`) that
    pass `app.tabs[idx].key()`; the flag-on `close_tab` (`:10854`) and
    `select_tab` (`:10860`) call no browser helper, and the frontend drives
    the webview through `browser_ensure` and `browser_hide`
    (`desktop-ui/src/lib/stores/browserHost.ts:23`, `:27`) → **P54**;
  - the view-state prune: with the flag on, the follower's `apply`
    (`tabs/follow.rs:85`) prunes it over the App's keys
    (`desktop.retain_tabs(&live)`, `:99`), and `persist_app_tabs`
    (`tabs.rs:55`, keys `:60`–`:61`) prunes it after `change_base`
    (`commands.rs:1433`), after `place_tab` (`:6355`) and on window close and
    exit (`main.rs:1918`, `:2240`) → **P54** (the registry's `TabsChanged`
    drives the prune); the calls in `force_refresh_diff` (`:3480`),
    `open_worktree_tab` (`:7320`), `set_active_project_impl` (`:10146`) and
    the flag-off tab commands are flag-off bodies;
  - `line_commits_for` (`commands/line_commits.rs:233`) resolves its target
    from `app.tab()` (`:240`–`:242`) → **P61**;
  - the export commands render `app.tab()` (`commands.rs:10668`, `:10685`,
    `:10709`), `reveal_er_folder` reads `app.tab().er_dir()` (`:2448`), and
    `open_in_vscode`, `open_source` and `open_in_editor` read the App's
    selection → **P61**;
  - the in-flight ask-AI replies (`AppState.pending_ai_replies`,
    `commands.rs:180`; `PendingAiReplies`, `snapshot.rs:2364`), which
    `build_ai_snapshot(tab, pending)` (`:4970`) merges into the AI snapshot as
    the "…thinking" reply → **P58.1** (served in `jobs`), **P58.2** (the
    flag-on delete paths remove entries) and **P60.2** (the writers);
  - arena `resolve` (`arena_service.rs:55`), with `arena_context`
    (`:26`–`:40`) reading `app.tab().ai.review`, and flow `key`
    (`flow_service.rs:39`) → **P60.1**;
  - the startup arena and flow wiring in the flag-on block at
    `main.rs:1137`–`:1141`: `flow_service::reconcile_on_load(&app, &service)`
    (`:1140`; `flow_service.rs:200`, iterating `app.tabs` at `:202`)
    recovers each tab's flow bucket → **P60.1** (from the registry's tab list
    and each tab's content); `arena_service::attach` (`:1139`;
    `arena_service.rs:238`) hands the App's arena recovery a liveness check
    → deleted in **P62.2** with the App's arena recovery, as is
    `arena_commands::attach_arena_notify` (`main.rs:1102`–`:1104`, both
    builds);
  - the PR head probe's target (`pr_head_probe_target`) → **P55.2**;
  - background threads in `main.rs` that read the App's active tab:
    - the active-branch watcher `run_active_branch_watcher` (`:702`) picks
      its target with `desired_local_branch_watch` (`:719`, `:2286`),
      mirrors the checkout root with `apply_watch_checkout_root` (`:753`),
      and on a file change calls `refresh_diff_quick_with_unmark` on the App
      tab (`:783`), so with the flag on `diff(tab)` never follows a file save
      and auto-unmark never reaches the reviewed section → **P56.1**;
    - the branch-base remote-oid probe (`:1275`, `probe_app.try_lock()`
      `:1280`), which feeds the stale pill for branch views → **P56.1**;
    - the GitHub status loop (`:1427`, `active_pr_identity(g.tab(), …)`
      `:1436`), which feeds `pr_status` → **P59**;
    - the inbox sweep's prefs (`commands::clone_inbox_prefs(&bg_app)` `:1650`,
      `:1683`; `commands.rs:465` reads `app.config.inbox`) and the setup
      hook's `commands::clone_inbox_prefs(&state.app)` (`:1957`, for
      `flush_pending_native_notifications`) → **P55.3**;
    - the meta-cache interval (`meta_app.try_lock()…tab().is_remote()`
      `:1749`) → **P55.1**;
  - `ai_selection_service.rs` (`:30`) and `gh_pr_actions.rs` (`:79`,
    `:101`) are the other files outside `commands.rs` that lock the App;
    their owners are above (P53 then P60.2, and P59);
  - the App follower (`tabs::follow_registry`, `tabs/follow.rs:57`, #578) →
    deleted in **P62.2**.

## Gate steps (coordinator, before P50 starts)

1. Done: #578 and #599 merged, and #538 is closed with the exit checklist and
   the "left on the App" list in its body; STATUS and BUILD_LOG updated
   (`5bafa5e53`).
2. Done (**D2**): #600 merged `main` into `release/v1.0.0` at `1f7527a1f`.
   No entry holds it.
3. Done: every citation here and in the step plans is pinned at
   `1f7527a1f` (see Notes).
4. The coordinator confirms the proposals below, the owner answers the
   pending questions or accepts their defaults, and the owner approves the
   plan; then the issues open as sub-issues of #371.
5. The phase-5 gate is logged in BUILD_LOG (PLAN's gate step 6).

## Decisions settled by the coordinator

Each A-numbered item is a planning proposal the coordinator confirms or
changes at the gate.

**Coordinator rulings (round 3, binding):**

- **`unmark_reviewed`: decision A** (posted on #538 and #599). P57's fix
  stands for all three reviewed writes (`toggle_reviewed`, `mark_reviewed`,
  `unmark_reviewed`): each carries the `TabKey` the page painted, and a
  mismatch is refused.
- **Saved-PR cap:** today's rule stays. The 51st save drops the oldest
  (`projects.rs:190`–`:199`, `MAX_PR_HISTORY` = 50, `:6`); nothing is
  refused.
- **Latency (A9) is report-only:** each entry records the old and new medians
  of its hot actions in the PR, and the reviewer acts on an overrun past the
  budget (within 10% or 2 ms of the old median, whichever is larger). No CI
  assertion and no latency plant.
- **The `head_oid` bug on `main`:** the owner decides whether a separate
  bug-fix PR goes to `main`. Either way P55.2 fixes the resource path, and no
  entry carries a `main` fix.

**Owner questions (pending).** The plan proceeds on each default until the
owner answers.

1. A10, P57, P54: the desktop's copy of selection, cursor, navigation and
   input mode moves in phase 5 (D20 had 5b). Default: yes, because P62.2
   removes the App from er-desktop; the App keeps them for the TUI until 5b.
2. A11: the use cases stay in er-engine until 5b (this narrows
   ADDENDUM-05's B1). Default: yes.
3. P62.2 and P61 crate-edge work: er-desktop drops er-engine's default
   features, a `check-desktop` CI job, the export renderer in the
   always-compiled half. Default: keep it in phase 5, since it is what makes
   "er-desktop has no App" enforceable.
4. An all-broken tab restore. Default: a Working tab on the launch root
   (P54).
5. The session-only AI pick (`persist: false`). Default: drop it; no
   desktop-ui caller uses it (`CommandPalette.svelte:313`, `:337`, `:349`
   and `AiSettings.svelte:79`, `:85`, `:91` all pass `persist: true`), so
   every pick saves (A18).

**Proposals:**


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
  er-protocol and names tabs by er-protocol's `TabKey(u64)` (P50), a wire
  twin of the engine's `TabKey` (`crates/er-engine/src/tab_key.rs`), whose
  field is `pub(crate)` and whose only constructor is `TabKey::next()`
  (ADR 0068). P52.1 adds one read accessor in lane S,
  `TabKey::get(self) -> u64`, and no constructor. The conversion lives in
  er-service as two free functions beside the registry: `wire_tab_key`
  (engine to wire, through `get`), and `resolve_tab_key(&TabsView, wire)`,
  which looks the value up in the registry's published tab list and answers
  that entry's engine key. A wire key is never turned into a new engine
  key, so a key the registry does not list, including one from before a
  restart, answers `ErErrorKind::NotFound`. A `From` impl between the two
  types fails in er-service and er-desktop alike (E0117: both types are
  foreign there), and putting it in er-engine would give the engine an
  er-protocol dependency for one conversion. The desktop adapter resolves
  the `TabContext` from the published content, as the phase-4 content
  routes do.
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
    actor's jobs, arena and flow sections, the auto-triage queue and the
    in-flight ask-AI replies; its version is a desktop counter the relay
    bumps when any part moves; first served in P58.1 with the in-flight
    replies, extended in P60.1 and P60.2),
    `session` (watch status, app version, active AI labels, config load
    error), `view(tab)` (browser pane, ADR 0068) and `panels`, and
    `ui_annotations(tab)`.
  - Notices become an `er://notice` event: a notice happens once and has
    nothing to refetch.
  - A fetch never waits on an actor's queue (ADR 0067). A loaded key
    answers from the published view; a workspace key the registry lists but
    no actor has loaded sends `LoadTab` without waiting (starting the actor
    when none runs) and answers an empty resource at version 0, and the push
    brings the content; a key the registry does not list answers
    `NotFound`. The one bounded wait is A4's `ParseFiles`.
- **A4 Content addressing.** `hunks(hash)` is keyed by the hash of one file's
  own hunk lines, `file_lines_key`'s rule (ADR 0013), computed by the service
  builder. Threads leave the hunk payload: they come from `annotations(tab)`
  and DiffView joins the two by path and line, so a comment no longer
  re-sends a file. A hunks resource is immutable, so it has no version; the
  frontend keeps an LRU cache bounded by bytes. A fetch for a file the
  section has not parsed sends `ParseFiles` and waits within ADR 0067's bound,
  so there is still one parse path; the request names the `TabKey` and the
  path beside the hash, since a hash alone names no actor (P56.2). This deletes `SentFilesState`,
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
  return `Result<T, ErError>` or a type that is not a `Result`, and no path
  segment of its return type may end in `Snapshot`. Its
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
  entry records the old and new medians in its PR, and the reviewer acts on
  an overrun past the budget (the paint stays pending until ack, or the entry
  does not merge until the number is accepted). No CI assertion and no
  latency plant (coordinator ruling). P52.1 extends the harness to
  er-desktop, with the old-path baseline taken over N runs on a 5,000-line
  diff before any store moves, for all seven hot actions below. The hot actions and the entry that measures
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
  D20 says selection, cursor, scroll, overlays and input mode move in phase
  5b. The desktop's copy moves earlier, in P57 (and P54 drops `input_mode`),
  because P62.2 deletes the App from er-desktop, so the desktop cannot keep
  reading them from it; the App keeps those fields and methods for the TUI
  until 5b. This changes D20's timing for the desktop only; it is owner
  question 1.
- **A11 Use cases stay in the engine.** ADDENDUM-05's B1 moves the use cases
  into er-service "when the old path is deleted in phases 5 and 5b". The TUI's
  `App` still calls them until 5b, and er-service depending on er-engine means
  they cannot move up while an `App` caller remains. They move in 5b (owner
  question 2).
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
- **A17 The backend tick.** `poll_impl` (`commands.rs:11986`) runs only when
  the frontend calls `poll`, which it does on each `er://revision` event and
  on the 30 s safety net. P52.1 adds a flag-on backend tick in er-desktop that
  sends `ReloadSidecars` for every loaded key (today's
  `content_routes::reload_sidecars` covers the active tab only) and runs
  `poll_impl`'s other first-pass duties: `drain_agent_log`,
  `check_commands`, `poll_background_tasks`, `process_ai_task_inbox`,
  `check_ai_files_changed` and `kick_worktrees_warm`. With the flag on,
  `poll_impl` skips that first pass and its `reload_sidecars` call
  (`commands.rs:11990`–`:11991`), so each duty has one driver. The tick keeps
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
  `tabs` in P55.1; `drain_agent_log`, `check_commands`,
  `poll_background_tasks` and `process_ai_task_inbox` move off the App with
  the runs they drive in P60.2; and `check_ai_files_changed` stays until
  P61. It is the only path that brings outside sidecar writes into the
  App's `tab.ai` (`commands.rs:12024`, and the comment at `:12002`–`:12005`;
  `TabState::check_ai_files_changed`,
  `crates/er-engine/src/app/state/mod.rs:3405`), and flag-on code reads
  `tab.ai` from the App until P61: the snapshot's `ai` field until P58.1,
  the GitHub senders until P59, `arena_context`
  (`arena_service.rs:26`–`:40`, `app.tab().ai.review`) until P60.1,
  `ask_ai_impl` and `ask_ai_for_finding` (their thread and finding lookups,
  `commands.rs:5777`, `:5848`, `:6002`) and the `ai_assist` arm of
  `reply_to_finding` (`reply_to_finding_impl`, `tab.ai.review` at `:10458`)
  until P60.2, and the export commands (`crates/er-desktop/src/export.rs:55`,
  through `render_markdown(app.tab(), …)`, `commands.rs:10668`) until P61,
  the last in the chain. P61's Depends-on line names P58.1, P59 and P60.2
  (which brings in P60.1), so every other reader has left when it removes the
  duty. P52.1 adds the test
  that holds the duty there: with the flag on, an outside write to
  `questions.json` in a loaded tab, one tick, then `export_review`, and the
  export includes the new question; dropping the duty from the tick fails
  it. P61 removes the duty once the export renders from the content, and the
  same test then passes through `ReloadSidecars`. P62.2 deletes `poll`,
  after the tick exists.
- **A18 The session AI pick.** With the flag on, the AI selection has one
  source: the settings actor's default selection (ADR 0069), which
  `set_ai_selection` and `set_ai_effort` already save through
  `SetDefaultAiSelection` and `SetDefaultAiEffort` when `persist` is true.
  `list_ai_providers` and the `session` resource's AI labels read it from
  P53, arena and flow effort from P60.1, and the `ask_ai` runs from P60.2.
  `persist: false` is dropped on the flag path (owner question 5): the wire
  commands carry no persist flag, and every pick saves. Until the App's last
  flag-on reader of `current_ai_provider`, `current_ai_model` and
  `current_ai_effort` leaves in P60.2, P53's `er_send` mapping keeps
  writing the saved selection into those App fields, as
  `ai_selection_service::snapshot` does today (`ai_selection_service.rs:30`–`:34`);
  P60.2 deletes that write. The flag-off path keeps the session pick until
  P62.2.

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
  phase 5. Phase-4 exit = the service owns all tab state and routes. So
  phase 5 must render the desktop from the service's published views (tab
  content sections, tabs, settings, jobs, ...) through er-protocol
  resources, replacing the flag-on snapshot that still reads the App."
- "The owner re-confirmed both gate answers on 2026-11-17 and asked for: an
  addendum, issues with step plans, and owner approval at the gate before
  builds start."

## PR entries

Shared acceptance: `fmt-check` and `clippy` always; `test`, `test-desktop`,
`test-all` (er-service is outside `just test`; er-protocol joins it in P50), `test-v1` and
`build-service-headless` as the touched crates require; `build-mcp` when engine
features or dependencies change; `docs-check` for docs and ADRs; and for every
entry that touches desktop-ui, `check-ui`, `lint-ui` and `test-ui`. Lint only
tightens (ADR 0038). A held-App-mutex test asserts that the action happened
with the App locked (the target was picked, the command sent, the answer
returned): several sites use `try_lock` and skip when the App is held, so a
test that checks only a time bound passes against today's code. Each entry from P52.1 on runs `protocol-check` (P50).
Until P62, every store entry leaves the desktop working with the flag on and
with it off; `test-ui` runs each adapter store's tests in both modes. The stop
line is 1,500 added-plus-changed lines after rename detection; pure deletions
are measured by what remains and do not count (ADDENDUM-04's L5).

### P50 `er-protocol`, ts-rs codegen and the drift check

- Branch: `epic/p50-er-protocol`. Lanes: C, P. Model: `opus`. Depends on: #538 closed and this addendum's gate steps.
- **Goal:** the wire crate and generated TypeScript, before any resource uses them.
- **Done when:** `crates/er-protocol` exists as a leaf crate with `ResourceKey`, `ResourceVersion`, `Versioned<T>`, `ResourceChanged`, `Ack`, `ErError { kind, message }`, the wire `TabKey(u64)` (A2) and an empty wire command enum, each deriving serde and `TS`; the `u64` fields (`TabKey`, `ResourceVersion`) carry `#[ts(type = "number")]` if the pinned ts-rs maps `u64` to `bigint`, which JSON over Tauri IPC does not deliver (keys and revisions stay below 2^53); `just protocol-gen` runs the explicit generator (A16) into `desktop-ui/src/lib/protocol/generated/`; `just protocol-check` exports into a temporary directory and fails on any difference with the committed files, and CI runs it on the `rust` and `ui` change outputs; `protocol-check` joins `just ci`; the `er::core_io` manifest check covers er-protocol (serde, ts-rs) through a second allow-list beside er-core's `ALLOWED` (A1); er-protocol joins `just test`; a test under `crates/er-protocol/tests/` fails on any `ts(export` attribute in `src/` (outside `src/`, it cannot match its own needle), no `.cargo/config.toml` sets `TS_RS_EXPORT_DIR` (it would point a stray export at the tracked directory and hide it from the porcelain step), and CI's `test` job fails when `git status --porcelain` is not empty after `cargo test` (A16).
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
- **Done when:** `AppState` (`commands.rs:157`) is split: a new `DesktopState` (`crates/er-desktop/src/state.rs`) holds every field except `app` (`:163`) and the old path's `sent_files` (`:225`) and `last_sent_*` counters (`:211`–`:215`), and the modules phase 5 adds take `DesktopState`, so they cannot reach the App; `run_blocking` moves into `state.rs` too, so `github_image.rs` (which imports it from `commands` at `:14`) and the new modules do not depend on `commands.rs`; a desktop resource host maps each `ResourceKey` to its owner (workspace actors through `ServiceState`, the registry, the settings actor, the desktop owners); a relay subscribes to each actor's `ViewPublished` and to `TabsChanged`, compares section revisions with the last published view and emits `ResourceChanged` only for keys whose revision moved, coalesced as `run_revision_watcher` does; the three commands are registered in both builds (A2), in `crates/er-desktop/permissions/app-commands.toml` and in `ipcAllowlist.test.ts`, and pass `er::command_shape` with no allow-list entry; `impl From<WorkspaceError> for ErError` lives in er-service, which gains its er-protocol dependency (A2); `TabKey::get` (er-engine) and er-service's `wire_tab_key` and `resolve_tab_key` exist, and a wire key the registry does not list answers `NotFound` (A2); the first resource, `tabs`, is served from `AppRegistry::tabs_view()` (`app_registry.rs:190`) and never through `AppRegistry::send` (`:161`), which applies tab commands inline on the caller's thread; the backend tick sends `ReloadSidecars` for every loaded key and runs `poll_impl`'s other first-pass duties, which `poll_impl` skips with the flag on (A17), `check_ai_files_changed` included until P61 (A17); the latency harness is extended to er-desktop and the old-path medians for all seven of A9's hot actions (open target, save PR, mark read, tab switch, mark reviewed, next file, add question) are recorded in the PR as the baseline; the old `er://revision` path is untouched.
- **Planted breaks:** a relay that emits every key on every publish fails the precision test (an arena tick moves no `diff(tab)` version); a write that does not bump fails the bump-on-write test; a `tabs` fetch routed through `AppRegistry::send` instead of `tabs_view()` fails the read-path scan (the held-actor bound test is P56.1's, on `diff(tab)`, which an actor serves); a wire key resolved by anything but a registry lookup fails the unknown-key test; dropping `check_ai_files_changed` from the tick leaves an outside question out of the flag-on export and fails the export-reload test; a module under `resources/` other than `tick.rs` that names `AppState` or `er_engine::app` fails a source scan; dropping the command from `app-commands.toml` fails the allowlist test; with the tick disabled, an outside write to a sidecar in a loaded tab's `er_dir` leaves that tab's `AiSection` revision unmoved with no frontend call, and the reload test fails; a `poll_impl` that still runs its first pass or its `reload_sidecars` call with the flag on fails the one-driver test; a tick that ignores revision bumps leaves a finished agent command undrained until the interval and fails the wake test.
- **Fallback split:** P52.1a the `AppState` split; P52.1b host, relay, the wire key and the tick; P52.1c the commands, permissions and the harness.
- Size: L.

### P52.2 The frontend resource client, the adapter pattern and the protocol ADR

- Branch: `epic/p52b-resource-client`. Lanes: U, P. Model: `opus`. Depends on: P52.1.
- **Goal:** desktop-ui can hold resources by version, refetch on push, and run one adapter store in both flag modes.
- **Done when:** a hand-typed client wraps the three commands with the generated types; a resource store tracks the versions it holds, refetches a shown key only when an `er://resources` version exceeds it, and asks `er_versions` on its own 30 s safety net and once after the listener attaches; the snapshot's `poll` keeps running with the flag on beside it, for the stores not yet ported, until P62.1 (A17); the `tabs` adapter store reads `app.snapshot.tabs` with the flag off and `er_fetch` with it on, and the tab strip reads the adapter; the protocol ADR is added; `CONTEXT.md` gains **Resource** (a versioned piece of published state with one writer).
- **Planted breaks:** a store that refetches on every event fails the version test; a resource store that refreshes through `poll` instead of `er_versions` and `er_fetch` fails the flag test; a listener attached after a push leaves the store stale, and the attach-time `er_versions` test catches it when that call is removed; the tab strip reading `app.snapshot` directly fails a source scan.
- Size: M.

### P53 Settings, session chrome and panels

- Branch: `epic/p53-settings-resources`. Lanes: D, U, P (S only if a builder needs a settings-actor change; none is expected). Model: `opus`. Depends on: P52.2.
- **Goal:** the settings page, the AI picker and the app chrome read resources.
- **Done when:** `settings`, `risk(key)`, `session` and `panels` are served and their adapters read them; the settings writes, the AI selection writes and the risk reads go through `er_send`/`er_fetch` with the flag on; the AI selection follows A18: the wire commands carry no `persist` flag, so with the flag on every pick saves through the settings actor and `persist: false` is dropped (owner question 5), `list_ai_providers` (`commands.rs:5145`, today reading `current_ai_*` at `:5152`–`:5153`) and the `session` resource's AI labels (today `AppRead::session`, `snapshot.rs:3230`) read the settings actor's default selection, and the mapping keeps `ai_selection_service::snapshot`'s write of the saved selection into the App's `current_ai_*` (`ai_selection_service.rs:30`–`:34`) until P60.2 moves their last flag-on readers (`ask_ai`, `ask_ai_for_finding`, the `ai_assist` reply), so `ai_selection_service.rs` keeps its App handle for that one write; notices arrive on `er://notice`; the entry stops reading 14 `AppSnapshot` fields and 21 commands, listed in the issue.
- **Planted breaks:** a settings write that skips the actor fails the one-writer test (ADR 0069); a refused save whose rollback does not bump the version leaves the page on the refused value and fails the rollback test; a notice delivered twice fails the dedupe test; with the flag on, `set_ai_selection` through `er_send` and then an `ask_ai` (still on the App path until P60.2) runs with the new model, and dropping the App write from the mapping fails it; with the App locked, `list_ai_providers` answers the new selection, and reading `current_ai_*` from the App fails the held-App-mutex test.
- **Fallback split:** P53.1 `settings` and `risk(key)` with the settings and risk commands; P53.2 `session`, `panels`, the AI selection writes and `er://notice`.
- Size: M.

### P54 Tabs and the tab lifecycle

- Branch: `epic/p54-tabs-resource`. Lanes: S, D, U, P. Model: `opus`. Depends on: P53, #538 (#578 and P47.2c.2 merged).
- **Goal:** tab strip, opens and closes run on the `tabs` resource and `er_send`, and the phase-4 lifecycle gaps close.
- **Done when:** the tab commands and the opens (19 commands) send through `er_send` with the flag on, and none of them takes the App mutex; `tabs` carries labels and kinds; a tab's staleness keeps the snapshot's value in the adapter until P55.2 derives it from `prs(remote)`; a drop of a never-loaded key moves no version; a `LoadTab` for a key the registry no longer lists is refused; the closed-key set is bounded; an all-broken restore leaves a Working tab on the launch root; the launch root comes from the registry; Cmd-T on a remote PR tab opens a Working tab with a root; an already-open PR is selected whatever its tab kind (A15); the browser helpers `tab_browser` (`browser_webview.rs:230`), `sync_tab_browser_chrome` (`:244`) and `on_tab_selected` (`:343`) take the tab's `TabKey` instead of `engine: &App`, so `browser_webview.rs` names no App type outside its tests: their flag-on callers, `update_tab_browser` (`commands.rs:11563`; `app.active_tab` `:11576`, `set_active_tab_browser(&app, …)` `:11587`) and `cycle_tab_browser_layout` (`:11626`; `app.active_tab` `:11632`, `app.tab().key()` `:11635`), take the key from `tabs`, and the flag-off twins of `close_tab` (`:10910`) and `select_tab` (`:10943`) pass `app.tabs[idx].key()`; the flag-on `close_tab` and `select_tab` gain no webview call, since the frontend drives the webview through `browser_ensure` and `browser_hide` (`desktop-ui/src/lib/stores/browserHost.ts:23`, `:27`); the webview labels and `BrowserWebviewState` stay index-keyed (`:20`, `:25`), so the nine browser commands keep their parameters for P61; with the flag on, the view-state prune runs on `TabsChanged` over the registry's tab list, replacing today's two flag-on prunes over the App's keys: the follower's `apply` (`tabs/follow.rs:85`, `desktop.retain_tabs(&live)` `:99`) and `persist_app_tabs` (`tabs.rs:55`, keys `:60`–`:61`), which runs after `change_base` (`commands.rs:1433`), after `place_tab` (`:6355`) and on window close and exit (`main.rs:1918`, `:2240`); the open-target median is recorded against the A9 baseline; the entry stops reading `tabs`, `active_tab`, `browser` and `input_mode`. No desktop-ui component reads `input_mode` (only `snapshotChrome.ts:57` carries it over and `types.ts:793` declares it), so the flag-on desktop drops it and no resource carries it; the key handler's modes stay frontend-local, and the App keeps `input_mode` for 5b (D20).
- **Planted breaks:** a `DropTab` on a never-loaded key that bumps fails the version test; a late `LoadTab` after a close that applies fails the race test; an unbounded closed set fails the bound test after 10,000 closes; `same_pr` comparing kinds fails the cross-kind open test; restoring with every descriptor broken leaves no tab and fails; a `launch_root` that reads the App's startup tab fails the flag-on launch-root test; an open that locks the App fails the held-App-mutex test; with the App locked, a flag-on `update_tab_browser` and `cycle_tab_browser_layout` each update the active tab's browser state, keyed from `tabs`, and resolving the key from `app.active_tab` fails that test; with the App locked, a close's `TabsChanged` prunes the closed tab's browser state, and a prune that reads `app.tabs` fails.
- **Fallback split:** P54.1 the resource, commands and adapters; P54.2 the lifecycle fixes.
- Size: L.

### P55.1 The sidebar owner, `projects` and `branches(project)`

- Branch: `epic/p55a-sidebar-projects`. Lanes: DS, US, P, D (planned exception, see pair (a)). Model: `opus`. Depends on: P52.2.
- **Goal:** the desktop app owner exists and publishes the project rows and each project's branches.
- **Done when:** the owner module holds one writer per resource and bumps on write; `projects` carries the project records without PR lists, and `branches(project)` the local and auto branches with worktree paths and the open PR number, from the metadata cache; `is_active` and `is_current` are derived in the frontend from `tabs`; the project commands (7) go through `er_send`; on the flag path the startup registration registers from the registry's tab list instead of `sync_projects_from_tabs(&app.tabs)` (`projects.rs:472`) and `app.tab().repo_root`, at both of its sites: the block both builds run (`main.rs:1026`–`:1032`) and the flag-on block's second call after `tabs::restore` (`main.rs:1132`, inside `:1127`–`:1136`); the meta-cache loop picks its interval (`main.rs:1749`) from the `tabs` resource's active key instead of the App tab, and the tick's `kick_worktrees_warm` takes its target from the same key (A17); the entry stops reading `projects` (its non-PR fields) and `worktrees`.
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
- **Done when:** `inbox` carries the items, the unread count and the last refresh, computed on the server; the inbox commands (9, including `open_inbox_item` and `prefetch_inbox_item`) go through `er_send`; with the flag on, `inbox-read` and `inbox-clear` no longer paint (A8: rule moved to the server for the unread count, deleted for the rest), and `mergeGlobalConfirm` runs flag-off only; the inbox sweep and the setup hook read the inbox prefs from the settings actor's view instead of `commands::clone_inbox_prefs` on the App (the sweep's `&bg_app` at `main.rs:1650`, `:1683`; the setup hook's `&state.app` at `:1957`, before `flush_pending_native_notifications`), so the helper (`commands.rs:465`) has no flag-on caller; the mark-read median is recorded against the A9 baseline; the entry stops reading `inbox_items`, `inbox_unread_count` and `inbox_last_refresh_ms`.
- **Planted breaks:** an unread count computed in TypeScript fails a source scan; a mark-read that does not bump `inbox` fails; an `inbox-read` paint with the flag on fails the flag test; an inbox sweep that reads the App's config fails the held-App-mutex test; with the App locked, the setup hook's flush of pending native notifications reads its prefs, and `clone_inbox_prefs(&state.app)` fails the same test.
- Size: M.

### P56.1 `diff(tab)`, loading on activation, the warmer and the watcher

- Branch: `epic/p56a-diff-resource`. Lanes: S, D, U, P, C (C for the er-desktop dev-dependency only). Model: `opus`. Depends on: P54.
- **Goal:** the file list, mode, base and staleness of the active tab come from `diff(tab)`, and the background threads that refresh or probe the active tab key it through `tabs`.
- **Done when:** `diff(tab)` carries each file's row and content hash and the diff-level fields, in the file order the App shows, the mtime sort included (`TabState::mtime_cache`, left on the App by the 2a plan); fetching it for a key with no content sends `LoadTab`; a fetch follows A3's rule: a loaded key answers from the published view (`ServiceState::content`, `crates/er-desktop/src/service.rs:178`) and never waits on the actor, a listed key with no content sends `LoadTab` without waiting and answers an empty `diff(tab)` at version 0, which is below any published section, since an actor's revisions start at 1 (`WorkspaceState::bump`, `crates/er-service/src/workspace.rs:246`–`:249`, from 0 at `:160`), and an unlisted key answers `NotFound`; the held-actor bound test lives here, with er-service's `test-support` feature for `Command::Stall` and er-desktop's dev-dependency on it; the warmer loads unloaded registry tabs at idle through `LoadTab`, scoped by the registry; the refresh, mode, base, filter and sync commands (8) go through `er_send`; the visible-file list uses the pure `visible_files` (`crates/er-engine/src/visible_files.rs:40`, #595); on the flag path the active-branch watcher (`run_active_branch_watcher`, `main.rs:702`) takes its target from `tabs` and the content instead of `desired_local_branch_watch`, replaces `apply_watch_checkout_root` (`:753`) with the content's checkout root, and on a file change sends a new service refresh route with auto-unmark for that key, instead of `refresh_diff_quick_with_unmark` on the App tab (`:783`); it keeps writing `WatchStatusState`, so P53's `session` resource still bumps; the branch-base remote-oid probe (`main.rs:1275`) takes its target from `tabs`; the entry stops reading 17 fields, listed in the issue.
- **Planted breaks:** a followed stub that never loads fails the first-diff test; a warmer that loads the active tab twice fails the warmer test; a filter computed against the App's tab fails the source scan; a `diff(tab)` fetch routed through `send_and_wait` fails the held-actor bound test (ADR 0067); a stale pill that ignores `preview_head_oid` fails the staleness test; a watcher that refreshes the App tab only leaves the `diff(tab)` version and the `ReviewedSection` revision unmoved after a file write in the watched checkout, and fails; a branch-base probe that reads the App fails the held-App-mutex test.
- **Fallback split (the plan):** P56.1a the resource, the adapter and the routed commands; P56.1b load on activation and the warmer; P56.1c the watcher, its service refresh route and the branch-base probe.
- Size: L.

### P56.2 Content-addressed hunks, previews and file reads

- Branch: `epic/p56b-content-addressed-hunks`. Lanes: S, D, U, P. Model: `opus`. Depends on: P56.1.
- **Goal:** hunks are fetched by content hash and cached in the frontend; the server records nothing about what it sent.
- **Done when:** `hunks(hash)` is served per A4; DiffView joins hunks with threads from `annotations(tab)` (adapter-fed until P58.1); the frontend cache is an LRU bounded by bytes; previews, compaction in both directions and `list_diff_paths` (5 commands) run on the service: `toggle_compacted` (`commands.rs:1304`) expands through `ParseFiles { expand: true }` and re-compacts through a new service command (lane S), since `ParseFiles` only expands and keeps a file expanded across loads (`er-service/src/protocol.rs:244`–`:249`; `DiffSection::expanded`, `content.rs:237`), and the re-compact removes the path from `expanded` and publishes the file compacted again, so a later load keeps it compacted; and `request_file_preview`'s document preview check (the "Document preview context changed" guard, `commands.rs:1138`–`:1139`, `:1151`–`:1155`) compares the content's `preview_context_key` for the request's `TabKey` instead of the App tab's; a `hunks(hash)` request carries the `TabKey` and the path beside the hash, so a fetch for a file the section has not parsed reaches the one actor that sends `ParseFiles` (A4); with the flag on, no code path calls `record_sent_file` or reads `hunks_omitted`.
- **Planted breaks:** a hash that includes threads makes a comment re-send the file and fails the comment test; an unbounded cache fails the byte-bound test; a fetch of an unparsed file that parses on the reader thread instead of through `ParseFiles` fails the one-parse-path test; a flag-on call to `record_sent_file` fails the source scan; with the flag on, a compacted file expanded and then re-compacted is compacted in `diff(tab)` and stays so across a `LoadTab`, and a re-compact that leaves the path in `expanded` (or is dropped) fails the toggle test in the second direction; with the App locked, a flag-on `request_file_preview` still answers, and a guard that reads `app.tab()` fails the held-App-mutex test.
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
- **Done when:** `annotations(tab)` is built from the AI section with thread ordering and counts on the server; `ui_annotations(tab)` is served by the desktop and bumped by its writes; the sidecar reload watches the comments directory as well as `er_dir`, driven by P52.1's backend tick (A17) with no frontend call; the tick keeps `check_ai_files_changed`, since flag-on code reads the App's `tab.ai` until P61 (A17); `jobs` is first served here, with one part: the in-flight ask-AI replies (`pending_ai_replies`, `commands.rs:180`, on `DesktopState` since P52.1; `PendingAiReplies`, `snapshot.rs:2364`), keyed by thread or finding id as the map is today, with every insert and removal bumping `jobs`; with the flag on, DiffView and the AI cards join them with `annotations(tab)`, as `build_ai_snapshot(tab, pending)` (`snapshot.rs:4970`) and the hunk threads (`:2654`) merge them today; with the flag off the join does not run, because the snapshot's `ai` slice already carries the pending replies; P60.1 and P60.2 extend `jobs`; `load_tab` reads the sidecar stamps before the AI read, so an outside write between the two is not lost (#596 review); a write the workspace itself makes advances the AI section's sidecar stamp, where today the write keeps the old one (`content.rs:154`–`:155`, "the next reload reads this write back once"), so the tick's `ReloadSidecars` does not read the workspace's own write back as an outside change (#596 follow-up); a spawn failure in `SidecarReloads::start` (`er-service/src/sidecar_reload.rs:40`) removes the key from `running` (`:54`), so the next reload runs (#596 review); `annotations(tab)` carries the triage review update as a protocol type, so `TriageReviewUpdateSnapshot` (`triage_update.rs:13`) is flag-off only; the four read commands go through `er_fetch`; the entry stops reading `ai`, `tour`, `other_bucket` and `ui_annotations`.
- **Planted breaks:** one service `AddQuestion` moves the `annotations(tab)` version once across the next tick, and a write that keeps the old stamp moves it twice and fails the own-write test; an outside write to `questions.json` moves the `annotations(tab)` version with no frontend call, and the reload test fails when the comments directory is dropped from the watch or the tick is disabled; a stamp read after the AI read loses a write landed between them and fails the ordering test; a spawn failure that leaves the key in `running` makes the next reload answer false and fails; thread ordering done in TypeScript fails the source scan; an arena tick that bumps `annotations(tab)` fails the precision test; dropping the in-flight entry from `jobs` makes a flag-on `ask_ai` show no pending reply and fails the pending test; a removal that does not bump `jobs` leaves the "…thinking" reply showing after the answer lands and fails; the builder-parity fixture carries a pending reply, so a join that skips `jobs` fails it.
- **Fallback split:** P58.1a the builders, the reload and the first `jobs` part; P58.1b the desktop-ui port and the pending join.
- Size: L.

### P58.2 Annotation writes and pending until ack

- Branch: `epic/p58b-annotation-writes`. Lanes: D, U, P. Model: `opus`. Depends on: P58.1, P55.3 (both edit `optimisticLocal.ts`).
- **Goal:** the 27 annotation write commands go through `er_send` and the thread paints shrink to pending until ack.
- **Done when:** each write carries the `TabKey` and the view the page painted (today's `OptimisticView`, `snapshot.rs:165`, with the tab index replaced by the key: repo root, PR number, branch view and mode) and maps to the service command P41 added, by that key, with no App lock: a key the registry does not list answers `NotFound` (A2), and a painted view that differs from the content's is refused, as `route_annotation` (`commands.rs:6873`) refuses it today through `abort_wrong_view` on the App (`:6883`), the check that also guards the follower lag; this replaces the App-side check for the 14 commands routed through `route_annotation` and for the finding, checklist and UI annotation edits that still run `abort_wrong_view` on the App; `add_document_thread` (today `service_add_document_thread`, `commands.rs:2843`, reading the App tab's preview at `:2861`–`:2870` and `:2875`–`:2883`) compares the request's preview key with the content's preview key for its `TabKey`, as P56.2 does for `request_file_preview`, and refuses a changed document; the three lag tests in `commands/tests/tab_key_tests.rs` (`a_paint_is_refused_while_the_app_lags_the_registry` `:402`, `route_annotation_refuses_a_write_while_the_app_lags_the_registry` `:443`, `a_document_thread_is_refused_while_the_app_lags_the_registry` `:527`) get keyed successors here and keep running until P62.2 deletes them with `commands/`; `reply_to_finding` maps `ai_assist = false` only, and its `ai_assist` arm is P60.2's; the flag-on `delete_thread` and `remove_finding_thread` mappings call P58.1's owner removal for the thread's or finding's pending ask-AI reply (the old bodies remove it at `commands.rs:3065` and `:3125`), which bumps `jobs`; the 17 remaining `OptimisticOp` kinds are classified by name (A8). Pending until ack, with the `client_id`, the confirming fetch landing on the same row (ADR 0017): `add-thread`, `reply`, `promote`, `promote-finding` and `reply-finding` (its `existing` and `new` targets; the `ai` target paints nothing). Deleted with the flag on, the `annotations(tab)` or `ui_annotations(tab)` push painting the result: `resolve`, `delete-root`, `delete-reply`, `edit`, `dismiss-finding`, `resolve-finding`, `add-annotation`, `delete-annotation`, `checklist-toggle`, `remove-finding-thread`, `delete-finding-response` and `edit-finding-response`. Thread ordering, the one rule these paints applied, moved to the server in P58.1. The add-question median is recorded against the A9 baseline, and the deleted paints are timed click-to-paint with `profileLog`, numbers in the PR.
- **Planted breaks:** dropping `client_id` makes the confirmed row get a fresh id and fails the confirm test; a `resolve` still painted with the flag on fails the flag test; a `reply-finding` with a `new` target that does not paint pending fails the pending test; a write resolved to the registry's active tab instead of its own key lands on the other tab and fails the keyed-write test (the successor of `:402`); a write for a key the registry no longer lists that is not refused fails the unknown-key test (the successor of `:443`); dropping the view comparison lets a write painted in another mode land and fails the stale-view test; a document thread checked against the App tab's preview, or not checked, fails the document test (the successor of `:527`), and with the App locked a flag-on document thread still answers; a delete that skips the pending-reply removal leaves the "…thinking" entry in `jobs` and fails the delete test.
- **Fallback split:** P58.2a threads, comments and promote; P58.2b findings, checklist, artifacts and browser annotations.
- Size: L.

### P59 `pr(tab)`, `pr_status`, the stack and the GitHub senders

- Branch: `epic/p59-pr-resources`. Lanes: S, D, DS, U, P. Model: `opus`. Depends on: P58.1, P58.2 (the chain order: both hold D and U), P55.3 (DS is free once the sidebar track ends).
- **Goal:** the PR card, merge box, GitHub status and stack read resources, and no GitHub command picks its tab from the App.
- **Done when:** `pr(tab)` is built from the PR section; `pr_status(remote, number)` from the status cache through the desktop owner; the stack is part of `pr(tab)`: `PrSection` gains it, and a service command for the stack refresh runs `gh_stack::load_from` off the actor thread, as `refresh_stack_impl` (`commands.rs:3345`) runs it off the App lock, so the actor stays its one writer and a stack change moves the PR section's revision (decided: the stack is per tab, refreshed with the PR and read only by the PR card, so its own resource would add a fetch to every card render); `active_tab_command` (`commands.rs:6895`), `send_for_active_tab` (`:6904`) and the flag path's comment tick `spawn_service_comment_sync` (`main.rs:812`, which picks its PR with `app.try_lock()` and `active_pr_identity(guard.tab(), &pr_cache)` at `:823`–`:826` and its target with `active_tab_command(&guard)` at `:834`) take the tab from the `tabs` resource and its content (the document preview check sits inside `request_file_preview` and is P56.2's); `github_comment_builders.rs` imports `NewGitHubComment` from `er_engine::github_post_session` (`github_post_session.rs:29`) instead of `er_engine::app` (`:7`); it is the same type, which `er_engine::app` re-exports through `app/state/github_post.rs:22`–`:24` and `app/mod.rs:25`; the helpers in `gh_pr_actions.rs` that outlive `run_github_pr_action` take `DesktopState`, and `run_github_pr_action_blocking`'s App reads (`active_github_key(&app, state)` at `gh_pr_actions.rs:79`–`:80`, the snapshot at `:101`–`:102`) give way to the `tabs` resource and an `Ack`; the GitHub status loop (`main.rs:1427`) takes its PR identity from `tabs` and `prs(remote)` instead of `active_pr_identity(g.tab(), …)` (`:1436`); the 13 GitHub command functions (12 names; `submit_github_pr_decision` has two bodies) go through `er_send`; the entry stops reading `pr`, `github` and `stack`.
- **Planted breaks:** a GitHub post that reads the App's active tab fails the held-App-mutex test; a status loop that reads the App tab fails the same test; with the App locked, the comment tick still picks its PR from `tabs` and sends the sync, and the `try_lock` plant skips the tick and fails the same test; a status refresh that does not bump `pr_status` fails; a stack refresh that does not move the PR section's revision fails; a merge pinned to a stale head fails the head test.
- **Fallback split:** P59.1 `pr(tab)`, `pr_status` and the stack; P59.2 the GitHub senders and commands.
- Size: L.

### P60.1 Arena and review flows on `jobs`

- Branch: `epic/p60a-jobs-arena`. Lanes: S, D, U, P. Model: `opus`. Depends on: P58.2 (it extends `jobs`, which P58.1 first serves, and follows P58.2 in the workspace chain; P58.2 brings in P58.1 and P55.3). Runs beside P59 (pair (b)).
- **Goal:** the arena page and the flow strip read `jobs`, and arena and flow commands stop resolving their tab on the App.
- **Done when:** `jobs`, first served in P58.1 with the in-flight ask-AI replies, gains the live arena runs, the review flows and their progress across actors; arena `resolve` and flow `key` take the tab from the `tabs` resource, and `arena_context` (`arena_service.rs:26`–`:40`) reads the review's `diff_hash` from the tab's AI section instead of `app.tab().ai.review` (A17); on the flag path `arena_service.rs` and `flow_service.rs` take `DesktopState` and name no `er_engine::app` type (`arena_service.rs:12`, `flow_service.rs:12`) outside their flag-off arms and tests, so their other App reads get sources too: config and effort (`arena_context`'s `app.config` `:30`, `resolve_effort` `:44` with the App's `current_ai_effort` at `:48`, flow `context`'s `app.config` and `current_ai_effort` `:24`–`:25`) from the settings actor's view and its default selection (ADR 0069, A18), the buckets (`app.tabs…er_dir()` `:31`) and `flow_key`'s tab scan (`flow_service.rs:52`–`:56`) from the registry's tab list and each tab's content, and `arena_branch_ref` (`:29`) and `review_flow_inputs` (`flow_service.rs:28`) from the tab's content, with any step only the App has extracted into the always-compiled half (lane S), as 2a.1 did; the 20 arena and flow commands go through `er_send` and their reads through `er_fetch`, which retires the three `ArenaRunSnapshotWire` commands (`flow_get_run`, `arena_get`, `arena_override`); an arena run and a review flow on a remote PR tab key through `tabs`, covering #577's surviving plants 8 and 9; the startup recovery `flow_service::reconcile_on_load` (`flow_service.rs:200`, called in the flag-on block at `main.rs:1140`) recovers each tab's flow bucket from the registry's tab list and each tab's content `er_dir` instead of iterating `app.tabs` (`:202`), while `arena_service::attach` (`main.rs:1139`, `arena_service.rs:238`) stays for the App's arena recovery and is deleted with it in P62.2; `App.svelte:293`'s `er://revision` listener is replaced by the `jobs` resource; the entry stops reading `review_flow`, `active_arena_run`, `arena_runs` and `background_arena_runs`.
- **Planted breaks:** an arena start that resolves the App's tab fails the held-App-mutex test; #577's plants 8 and 9 (re-applied) fail the new remote-PR-tab arena and flow tests; a progress tick that does not bump `jobs` fails; the Deep cancel and orphan test from P28b/P32b is reused unchanged; a startup with a flow bucket on a restored tab recovers that flow with the flag on, and a `reconcile_on_load` that reads `app.tabs` (which hold only the startup tab until the follower runs) misses it and fails the startup-recovery test; an arena or flow context that reads `current_ai_effort` from the App fails the held-App-mutex test after a `set_ai_effort`.
- **Fallback split:** P60.1a the `jobs` aggregate and the arena; P60.1b review flows and the `App.svelte` listener.
- Size: L.

### P60.2 AI runs, triage and task logs on `jobs`

- Branch: `epic/p60b-jobs-runs`. Lanes: S, D, DS, U, P. Model: `opus`. Depends on: P60.1, P59 (the chain runs `{ P59 ∥ P60.1 } → P60.2`), P55.3 (DS is free once the sidebar track ends).
- **Goal:** card AI, review runs, triage and the background task panel read `jobs`.
- **Done when:** the 19 run, triage and log commands go through `er_send`/`er_fetch`; triage spinners read `jobs` (owner answer); auto-triage writes its queue through the desktop owner, and `auto_triage.rs` drops its App handle (`AutoTriageCtx.app`, `:17`) and `er_engine::app::BackgroundTaskTarget` (`:11`) for the service's job target, or for a target type moved to the engine's always-compiled half (lane S) if the service has none; `ask_ai` and `validate_with_ai` go through `er_send` and keep their in-flight replies in `jobs` (P58.1); `ask_ai_impl` and `ask_ai_for_finding` stop reading the App: provider, model and effort come from the settings actor's default selection (A18) instead of `current_ai_*` (`commands.rs:5834`, `:5835`, `:5880`, `:5881`, `:5907`, `:6112`, `:6113`, `:6151`), and the thread and finding lookups from the tab's AI section instead of `app.tab().ai` (`:5777`, `:5848`, `:6002`); with those readers gone, P53's write of the saved selection into the App's `current_ai_*` (`ai_selection_service.rs:30`–`:34`) is deleted and `ai_selection_service.rs` takes `DesktopState`; the `ai_assist` arm of `reply_to_finding` goes through `er_send`: today `FindingReplyRoute::FlagOff` (`commands.rs:10411`–`:10413`) sends it to `reply_to_finding_impl` (`:10437`), which locks the App, reads `app.tab().ai.review` (`:10458`), writes the stub thread on the App (`:10492`) and calls `ask_ai_impl` (`:10522`); the new mapping finds the finding in the tab's AI section, creates the finding thread through the service write P58.2 maps, then runs the ask through the new path with its in-flight reply in `jobs`. **Test change, up front:** `finding_reply_route`'s test (`commands.rs:16448`–`:16455`) asserts that `ai_assist` takes the flag-off route; it changes to assert that both values take the service route, and the `FlagOff` arm goes; the tick's four agent and task duties (`drain_agent_log`, `check_commands`, `poll_background_tasks`, `process_ai_task_inbox`) move off the App with the runs they drive, so the only App duty left on the tick is `check_ai_files_changed`, which P61 removes (A17); the entry stops reading `agent_commands`, `agent_log`, `background_tasks` and `bg_loading`.
- **Planted breaks:** a triage queued without a `jobs` bump leaves the spinner off and fails; a task log fetched from the App fails the source scan; a tick that calls an App duty other than `check_ai_files_changed` fails the same scan; an `er_send` `ask_ai` that skips the in-flight insert shows no pending reply and fails the pending test; with the flag on, `set_ai_selection` then `ask_ai` runs with the new model, and with the App locked it still starts, so an `ask_ai` that reads `current_ai_*` from the App fails the held-App-mutex test; with the App locked, an `ai_assist` reply creates the finding thread and shows its pending reply in `jobs`, and routing it to `reply_to_finding_impl` fails the same test; a cancel that does not reach the runner fails the reused `CancelJob` assertion (P43).
- **Fallback split (the plan):** P60.2a AI runs, triage, the `ai_assist` reply and the selection source on `jobs`; P60.2b agent commands, logs, background tasks and the tick's App duties.
- Size: L.

### P61 Window-mechanics commands typed and moved out of `commands.rs`

- Branch: `epic/p61-window-commands`. Lanes: D, S, U, P, C. Model: `opus`. Depends on: P55.3, P57 (desktop-ui owns the selection the open commands now take as parameters), P58.1 and P59 (A17: removing `check_ai_files_changed` needs every other flag-on reader of the App's `tab.ai` gone, and the snapshot's `ai` field leaves in P58.1 and the GitHub senders in P59), P60.2 (it holds D and U, so it runs after both tracks; it brings in P60.1).
- **Goal:** the 27 commands that stay direct return `Result<T, ErError>` with protocol types, and none of them lives in a file P62.2 deletes.
- **Done when:** the browser webview (9), terminal (4), OS launch and reveal (6), export (3), `start_window_drag`, `detect_dev_url`, `line_commits`, `fetch_github_image` and `dev_log_filter` commands are typed; the ones in `commands.rs` (15), `commands/line_commits.rs` and `arena_commands.rs` (`dev_log_filter`) move into a new `crates/er-desktop/src/direct/` module, one file per group (window, OS launch and reveal, terminal, export, line commits, dev log), and take `DesktopState` (P52.1), so P62.2 can delete `commands.rs`, the `commands/` directory and `arena_commands.rs` whole; `browser_webview.rs` and `github_image.rs` stay where they are; `open_in_editor`, `open_in_vscode` and `open_source` take the path and line from desktop-ui in both builds, since desktop-ui knows the selection in both (today they read `tab.selected_diff_file()` and `current_hunk`, `commands.rs:1678`–`:1693`); `reveal_er_folder`, `line_commits` and the export commands take the `TabKey`, and each keeps a flag-off twin on the App path until P62.2, because er-service is an optional dependency (`v1-service = ["dep:er-service"]` in `crates/er-desktop/Cargo.toml`) and a flag-off build has no `TabContent`: with the flag on, `reveal_er_folder` reads `er_dir` from the content (flag off: `app.tab().er_dir()`, `:2448`), `line_commits` resolves its target from the content's diff section (flag off: `resolve(app.tab(), &path)`, `commands/line_commits.rs:240`–`:242`), and the export commands render the content (flag off: `render_markdown(app.tab(), …)`, `commands.rs:10668`, `:10685`, `:10709`); the flag-on export takes er-protocol's export options with the same JSON shape in place of `ExportOpts`, which flattens the `ui`-gated `er_engine::export::ExportOpts` (`export.rs:14`, `:22`), and both arms share one renderer: the engine's markdown rendering (`render_markdown`, `crates/er-engine/src/export.rs:88`) and its options type (`er_engine::export::ExportOpts`, `:23`) are extracted into the always-compiled half (lane S, as 2a.1 extracted steps) over exactly what the renderer reads, and no diff rows: the branch name (`tab.current_branch`, `:89`–`:93`), the AI state (`tab.ai`'s questions, notes, GitHub comments, review and checklist, `:109`, `:134`, `:159`, `:183`, `:220`) and the comments directory, from which it loads the UI annotations (`load_ui_annotations(&tab.comments_dir())`, `:208`); the flag-on arm takes the branch from the content's `ResolvedTab` (`TabContent.tab.current_branch`; `crates/er-engine/src/tab_identity.rs:142`), the AI state from the content's AI section and the comments directory from its context (`TabContent.context`, `er-service/src/content.rs:33`; `TabContext::comments_dir`, `er-service/src/protocol.rs:488`); er-desktop's exclusion step (`resolve_exclusions`, `crates/er-desktop/src/export.rs:53`, which takes `&TabState`) gets a `TabState`-free signature over the same AI state and stays in er-desktop; `er_engine::export` keeps a `ui`-gated `render_markdown(tab)` wrapper and a re-export of the moved options type, and er-desktop maps er-protocol's `ExportOptions` onto the moved type; the export parity test compares the two arms; the tick's `check_ai_files_changed` goes, since the export was the last flag-on reader of the App's `tab.ai` (A17), and P52.1's export-reload test now passes through `ReloadSidecars`; `export_to_agent` stops returning `AppSnapshot` and its run shows in `jobs` (P60.2); the names among them that fail the rule leave the allow-list; their callers use the generated types.
- **Planted breaks:** restoring one to `Result<_, String>` fails `er::command_shape`; leaving a typed name on the allow-list fails as stale (P51), and so does re-adding one; an `open_source` that reads the App's selection fails the held-App-mutex test; an export rendered from `TabState` fails the export parity test against the flag-off output, whose fixture has a non-empty branch and a UI-annotation file in the comments directory, so a flag-on arm that drops the branch header (plant) or the UI-annotation section (plant) fails it as well; with the App locked, a flag-on `line_commits`, `reveal_er_folder` and `export_review` each answer, and a flag-on arm that resolves from `app.tab()` fails the held-App-mutex test; removing `check_ai_files_changed` before the export moves fails P52.1's export-reload test.
- **Fallback split:** P61.1 the move and the typing; P61.2 the flag-on arms of `reveal_er_folder`, `line_commits` and the export, the renderer extraction and the tick duty.
- Size: L.

### P62.1 Delete the old store layer

- Branch: `epic/p62a-delete-old-store-layer`. Lanes: U, P, C, D. Model: `opus`. Depends on: P53, P54, P55.1, P55.2, P55.3, P56.1, P56.2, P56.3, P57, P58.1, P58.2, P59, P60.1, P60.2, P61.
- **Goal:** desktop-ui runs only on resources.
- **Done when:** the adapters lose their flag-off branch; the `snapshot` store, `poll` handling, `optimisticLocal.ts`'s remainder, `snapshotDelta.ts`, `snapshotChrome.ts`, `tabSnapshotCache.ts` and `v1ServiceFlag.ts` are deleted; the hand-written `types.ts` is replaced by a generated barrel that `protocol-check` covers, and frontend-only types move to their modules; `scripts/tauri-dev.sh` drops the flag; er-desktop makes `v1-service` a default feature, so a backend built without it cannot pair with the new frontend. **Test change, up front:** P38's default-build assertion in the bin target (`!cfg!(feature = "v1-service")`, `crates/er-desktop/src/main.rs:2388`) flips to assert the feature is on, and from here `test-desktop` runs the flag-on path, which makes `test-v1` redundant for er-desktop; the flag-off backend tests compile out with their `cfg`.
- **Planted breaks:** a hand edit of `types.ts` fails `protocol-check`; a component reading `app.snapshot` fails the source scan; `check-ui` fails on a removed field still referenced; running `cargo test -p er-desktop --bin er-desktop --no-default-features` fails the flipped assertion (a build alone runs no assertion).
- Size: M (deletions do not count).

### P62.2 Delete the old desktop path and the desktop flag

- Branch: `epic/p62b-delete-old-desktop-path`. Lanes: D, DS, C, P, U. Model: `opus`. Depends on: P62.1.
- **Goal:** the phase exit.
- **Done when:** `snapshot.rs` and `commands.rs` are deleted, with the `commands/` directory (`content_routes.rs`; `line_commits.rs` left in P61), the App-backed desktop route, `get_snapshot`, `poll`, `v1_service_enabled`, `run_revision_watcher`, the flag-off bodies of the three resource commands (A2) and the follower (`tabs/follow.rs`, `follow_registry` `:57`); every App site that survives outside the deleted files is removed or was made App-free in the entry the step plan names, `AppState` included, with the startup wiring the App's arena recovery needs (`arena_service::attach`, `main.rs:1139`; `arena_commands::attach_arena_notify`, `:1102`–`:1104`), and `run_blocking` already lives in `state.rs` (P52.1); the old commands outside `commands.rs` go too, each replaced by `er_send`/`er_fetch` in the entry named: `arena_commands.rs` whole (19 commands, `:135`–`:667`, P60.1; `dev_log_filter` left in P61), `config_commands.rs` whole (11, `:32`–`:128`, P53), `risk_commands.rs` whole (2, `:17` and `:29`, P53) and `gh_pr_actions.rs`'s `run_github_pr_action` (`:64`, P59); any helper an `er_send` mapping still calls moves into the module that calls it in that entry; the two `*Snapshot` types outside `snapshot.rs` go: `SettingsSnapshot` (`settings.rs:12`) with `GetConfigHubResponse` (`:20`), replaced by P53's `settings` resource, and `TriageReviewUpdateSnapshot` (`triage_update.rs:13`), replaced in P58.1; er-desktop has no `er_engine::app` reference and depends on er-engine with `default-features = false, features = ["watch"]` (the watcher, `main.rs:707`); P61 put the flag-on export on a renderer in the engine's always-compiled half, so deleting the flag-off export twins and `export.rs`'s `ui`-gated imports (`:13`, `:14`) leaves er-desktop with no `er_engine::export` reference, and its dev-dependency becomes `default-features = false, features = ["test-support"]`, so a test build does not turn `ui` back on; a new `check-desktop` recipe (`cargo check -p er-desktop`, in the style of `build-service-headless`) joins `just ci` and a CI job with `tauri-linux-deps`, because resolver 2 unifies er-engine's features with er-tui in every workspace build, so `clippy --workspace` and `test-desktop` alone would still compile `ui`; the #578 follower notes deferred here (the gating scan's `cfg(any(…, test))`, the remote-PR row check that cannot fail, m02) close with the follower; the `v1-service` feature leaves er-desktop and stays on er-tui (A12), and `test-v1`, CI and the lint gate's second pass keep only er-tui; the `er::command_shape` allow-list is empty and deleted, with P51's frozen fixture, and no command returns a `*Snapshot` type or `Result<_, String>`; ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034 are marked "Superseded by" the protocol ADR, and docs-check fails when a superseded mark and the superseding ADR's list disagree; `CLAUDE.md`'s push-driven line, `crates/er-desktop/agent.md`, `desktop-ui/agent.md` and `desktop-ui/src/lib/stores/agent.md` describe resources; `app-commands.toml` and `ipcAllowlist.test.ts` list the surviving commands only.
- **Planted breaks:** a command returning `Result<AppSnapshot, String>` fails `er::command_shape` with no allow-list to hide in; a `use er_engine::app::App` or a call to `er_engine::export::render_markdown` in er-desktop fails `check-desktop` locally and in CI; deleting 0012 from the protocol ADR's supersedes line fails docs-check; a compile error behind `cfg(feature = "v1-service")` in er-tui still fails CI on a throwaway draft PR (#470's shape); the flipped `cfg!` assertion goes with the feature, and a test that reads er-desktop's manifest fails when a `v1-service` feature is re-added.
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
  - The Depends-on lines, which become each issue's Blocked-by line, follow
    this chain: P60.1 depends on P58.2, P60.2 on P60.1 and P59, and P61 on
    P58.1, P59 and P60.2, so A17's duty removal in P61 cannot merge before
    any other flag-on reader of the App's `tab.ai` has left.
- **Pair (a), the two tracks, a planned exception.** The sidebar entries
  hold D for these edits only, each conflict-by-addition or in a block of
  `main.rs`'s setup function that no workspace entry edits:
  - every sidebar entry: a dispatch arm per sidebar key in the resource host;
  - P55.1: the startup registration block (`main.rs:1026`–`:1032`) and the
    flag-on block's call at `:1132`, the meta-cache interval (`:1749`) and
    the tick's `kick_worktrees_warm` target (A17). The registration block
    ends five lines above P56.1's warmer scope (`:1037`), and `:1132` sits
    eight lines above P60.1's `reconcile_on_load` call (`:1140`), so
    whichever entry of each pair merges second rebases there by hand;
  - P55.2: the PR head probe thread (`:1357`);
  - P55.3: the inbox sweep's prefs (`:1650`, `:1683`) and the setup hook's
    (`:1957`);
  - the workspace track's `main.rs` blocks are P54's `launch_root`
    (`:1088`), P56.1's watcher (`:702`), branch-base probe (`:1275`) and
    warmer (`:1765`), P59's GitHub status loop (`:1427`), and P60.1's
    startup flow recovery (`:1140`);
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
   reviewed writes, and P58.2 for annotation writes (keyed, with the painted
   view checked against the content); #578's App-side check covers
   annotations until then; the rest close by deletion.
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
  `v1_service_enabled`. The counts hold at `1f7527a1f`: #578, #599 and #600
  added no command and changed no return type.
- **The `head_oid` gap on `main`:** `build_projects_cache_key`'s fingerprint
  is live on `main` (v0.6.x). The owner decides whether a separate bug-fix PR
  goes to `main` (coordinator ruling); P55.2 fixes the resource path only.
- **Re-pin (gate step 3).** Every line number moved from `949eb993d` (or the
  #578 head `88c21f507`) to `1f7527a1f` by mapping each cited line through
  `git diff` and checking that the line's text is unchanged; the files that
  moved are `commands.rs` (+147 lines), `commands/content_routes.rs`,
  `main.rs` (+41), `snapshot.rs` (+3), `service.rs`, `tabs.rs` (+5),
  `view_state.rs`, er-service's `actor.rs`, `app_registry.rs`,
  `content.rs`, `content_tests.rs`, `effect.rs` and `workspace.rs`, and
  er-engine's `app/state/mod.rs`. One site changed shape: the flag-off
  restore loop that P62.2 deletes is now `restore_app_tabs` (`main.rs:898`,
  with a flag-on stub at `:936`). The "left on the App" list above is
  reconciled with #538's closing body.
- Phase 5b and phase 6 are out of scope: the TUI keeps the `App`,
  `navigation.rs`'s `App` methods and the `TabState` view fields (D20);
  inbox, projects, MCP and the CLI move in phase 6.
