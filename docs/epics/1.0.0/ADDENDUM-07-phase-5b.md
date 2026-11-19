# Addendum 07: phase 5b, the TUI on the service

Draft for the owner's review. The owner approved writing it beside phase 5's
builds. It follows [PLAN.md "Re-planning at a phase
gate"](./PLAN.md#re-planning-at-a-phase-gate) as far as a plan written early
can. Where this file disagrees with PLAN.md or an issue body, this file wins
once committed. [ADDENDUM-01](./ADDENDUM-01-decisions.md),
[ADDENDUM-02](./ADDENDUM-02-issue-decisions.md),
[ADDENDUM-05](./ADDENDUM-05-phase-4.md) and
[ADDENDUM-06](./ADDENDUM-06-phase-5.md) still bind.

Every citation below was checked against `release/v1.0.0` at `ef41feb1c`
(2026-11-19), the status commit after P53.1 and P55.1a merged. Citations of
phase-5 sites name the symbol; ADDENDUM-06 pinned its own line numbers at
`1f7527a1f`, and they are not copied here. Symbols are the anchor; line
numbers are a convenience.

PLAN's phase-5b milestone:

- Goal: the TUI runs on the service with its own `TuiView`.
- Exit criteria: no TUI code mutates domain state directly; the TUI flag and
  the old path are deleted; TUI review submission is restored, since D3
  deferred it.
- Known risk: key handling that relied on `App` fields.

Because phase 5 is still building, the gate steps of PLAN (every phase-5 issue
closed, `main` merged, `just ci` green) cannot run yet. Builds start when the
owner approves this plan; each entry that depends on a phase-5 entry names it.

## Hand-offs from earlier addenda

Each item below was left for 5b by an earlier plan. Each has an owner here.

- **D3** (ADDENDUM-01): the TUI cannot submit a review with a decision after
  P05 and P06. The 1.0.0 release notes say so unless 5b restores it. PLAN's
  P05 put "a TUI pending-review UI" in 5b. Owner: P68.
- **D20** (ADDENDUM-02): selection, cursor, scroll, overlays and input mode
  move in 5b, with `navigation.rs`. ADDENDUM-06's owner answer 1 moved the
  desktop's copy into phase 5 (P54, P57), and the App keeps the fields and the
  `navigation.rs` methods for the TUI. Owners: P65 (the TUI's `TuiView`),
  P77 (the App's copy is deleted).
- **ADDENDUM-05 P46**: `file_tree_width` and `panel_width` stay for 5b with
  the TUI layout group. Owner: P65.
- **ADDENDUM-05 A15 and risk 13**: `SessionState` becomes the TUI's view file
  in 5b, and a new store kind for a view file is a 5b check. `session.json`
  is already a registered kind (`crates/er-engine/src/store_registry.rs:257`),
  so 5b adds no kind. Owner: P65.
- **ADDENDUM-05 B1 and ADDENDUM-06 A11**: the use cases stay in the engine's
  always-compiled half until 5b, and move up into er-service when the old path
  goes. Owner: decision 9.
- **ADDENDUM-06 A10 and P57's ADR 0068 consequence**: the Rust navigation
  functions stay for the TUI, held to desktop-ui's copy by a shared JSON
  fixture. Owner: P65, which calls those functions.
- **ADDENDUM-06 A12 and P62.2**: the `v1-service` feature leaves er-desktop
  and stays on er-tui; `test-v1`, CI and the lint gate's feature pass keep
  er-tui only. Owner: P76.
- **ADDENDUM-06 A1**: the service builders sit in er-service so the TUI and MCP
  can reuse them. Under decision 2 the TUI reads the published views directly
  and uses no er-protocol builder, so this reason now holds for MCP only.
- **ADDENDUM-06 P58.2 and P61**: `cleanup_triage`, `cleanup_review_artifacts`
  and the export renderer move to the always-compiled half and stay
  re-exported from `er_engine::app` "for the flag-off path and the TUI".
  Owners: P72 and P75 (the flag path calls the moved code), P77 (the
  re-exports go).
- **ADDENDUM-05 risk 6**: the service must not read the TUI's view state.
  Owner: P65, whose flag-on source scan keeps view state out of every
  command.
- **ADDENDUM-05 risk 4**: the TUI has no library target, so its key paths are
  tested only through the App. Owner: P64, whose key table is tested without
  the App.
- **PLAN phase 4 open question**: "whether the TUI keeps compiling against the
  old `App` until 5b". It does, and the flag-off TUI keeps running on the App
  until P76.

## Claims checked against the tree

Method: `git grep` and file reads at `ef41feb1c`.

- **Size and shape.** er-tui is 14,866 lines in 17 files, tests included, with
  122 tests. It builds one binary and has no library target
  (`crates/er-tui/Cargo.toml`, `[[bin]]`).
- **The flag compiles nothing yet.** er-tui depends on er-engine with default
  features, so `ui` is on (`Cargo.toml:15`), and on er-service as an optional
  dependency (`:17`, feature `:40`). No file under `crates/er-tui/src` names
  `feature = "v1-service"`, so today the flag-on TUI is the flag-off TUI.
- **Rendering reads the App.** `ui::draw` takes `&App`
  (`crates/er-tui/src/ui/mod.rs:18`). Each top-level renderer takes `&App`,
  and the helpers below them take `&App` or `&TabState`. Outside tests the
  `ui/` modules read about 72 distinct `TabState` members and 16 `App`
  members.
- **Keys drive the App.** The loop (`crates/er-tui/src/main.rs:510`) routes
  each key by `app.overlay` and `app.input_mode` (`:523`–`:537`) to handlers
  in `input/` that call App methods. Outside tests, `input/` and `main.rs`
  name about 240 distinct App and `TabState` members. This is PLAN's "key
  handling that relied on `App` fields".
- **View types live in the App.** The TUI imports `OverlayData`, `InputMode`,
  `ConfirmAction`, `HubAction`, `HubItem`, `HubKind`, `ConfigEditState`,
  `SplitSide`, `DirEntry`, `Worktree` and `CommandStatus` from the
  `ui`-gated `er_engine::app` (`input/mod.rs:3`–`:4`, `input/normal.rs:4`,
  `main.rs:14`, `ui/overlay.rs:9`, `ui/settings.rs:9`). The hubs are built by
  App methods; `HubAction` is at `crates/er-engine/src/app/state/mod.rs:396`.
- **The loop's duties.** Watcher refresh with auto-unmark (`main.rs:559`), the
  sidecar poll (`:579`), `check_commands`, `poll_background_tasks` and
  `drain_agent_log` (`:585`–`:591`), the watched-file rescan (`:595`),
  stacked-PR lookups (`:617`–`:628`), model discovery (`:631`–`:648`) and
  the debounced session save (`:681`, `:707`). At startup: `--remote`
  (`:228`–`:280`), `--target` (`:290`–`:295`), `--pr` (`:298`–`:312`),
  `--filter` (`:315`–`:317`), session restore (`:321`) and the PR probe thread
  (`:331`–`:362`).
- **er-tui writes domain state itself, outside the App.** `input/mod.rs` runs
  `git push` through the port (`:952`), deletes a watched file with
  `std::fs::remove_file` (`:972`), approves a PR through
  `github::gh_pr_approve` (`:1039`), and clears sidecars through
  `cleanup_questions_and_notes` (`:965`), `cleanup_reviews` (`:997`,
  `:1007`), `cleanup_question_answers` (`:1019`) and `cleanup_note_replies`
  (`:1028`). Those four clear helpers are `ui`-gated
  (`crates/er-engine/src/app/state/mod.rs:8454`, `:8490`, `:8505`,
  `:8522`), next to the two that P58.2 moves (`cleanup_triage` `:8527`,
  `cleanup_review_artifacts` `:8538`).
- **A render reads git.** The watched-file view calls
  `app.ports.git.diff_watched_file_snapshot` during the draw
  (`ui/diff_view.rs:2880`).
- **TUI-only writes with no service command.** Stage and unstage
  (`toggle_stage_file`, `app/state/mod.rs:8226`), stage all (`:8263`), commit
  (`submit_commit`, `app/state/comments.rs:1883`), push (above), the watched
  file snapshot (`update_watched_snapshot`, `mod.rs:4245`), the watched-file
  delete (above) and context expansion (`expand_context`,
  `app/state/navigation.rs:462`, through `git::refetch_file_with_context`).
  Neither er-desktop nor er-service does any of these.
- **What the service already covers.** The `Command` enum
  (`crates/er-service/src/protocol.rs:27`) has the reviewed, thread,
  promote, GitHub comment, sync, job, arena, flow and tab-content commands.
  The pending review has its four commands (`GetPendingReview` `:181`,
  `AddPendingReviewComment` `:183`, `RemovePendingReviewComment` `:189`,
  `SubmitPendingReview` `:194`), beside `SubmitGithubPrDecision` (`:211`).
  The settings actor has config patches, the default AI selection, model
  refresh and provider edits (`SettingsCommand`, `settings.rs:41`). The
  service relocates comments on load (`effect.rs:358`).
- **The published content uses engine types.** `TabContent`'s sections hold
  `DiffFile`, `AiState` and `PrOverviewData` (`content.rs:16`–`:19`;
  `DiffSection.files` `:233`, `AiSection.state` `:271`). The TUI's renderers
  already draw those types, so they can draw the content with no wire type.
- **The actor host is desktop code.** `ServiceState`
  (`crates/er-desktop/src/service.rs:28`) holds one actor per workspace key,
  the settings actor and the one app registry. er-service has no shared host.
- **The registry persists.** `AppRegistry` loads `tabs.json` on first use
  (`tabs_view` `app_registry.rs:221`, `loaded_tabs` `:257`–`:259`), saves it
  on every tab command (`:298`), and loads and saves `pr-open-cache.json`
  (`:318`, `:325`). A TUI on this registry would open the desktop's saved tabs
  and overwrite them. P54 adds a refusal of `LoadTab` for a tab key the
  registry does not list (#607), so a TUI tab must be listed in some registry.
- **Tab keys.** `TabKey::next` is public (`crates/er-engine/src/tab_key.rs:19`).
  Keys are process-local and never saved (ADR 0068).
- **er-service has no `test-support` feature** (`crates/er-service/Cargo.toml`).
  P56.1 adds it for `Command::Stall` (#611).
- **Pure helpers exist.** `er_engine::navigation` (`next_file`
  `navigation.rs:33`, from P47.1b), `er_engine::visible_files`, and
  `session_store::save_session` (`session_store.rs:152`) all compile without
  `ui`.
- **`jobs` is a desktop aggregate.** ADDENDUM-06 A3 builds it in the desktop
  adapter from each actor's jobs, arena and flow sections. The TUI cannot read
  it, so it builds its own rows from `WorkspaceView` and the job events.
- **The App is large.** `crates/er-engine/src/app` is 33,483 lines;
  `app/state/mod.rs` alone is 16,644.
- **er-mcp builds the engine headless** and calls always-compiled modules
  directly (`sidecar_upload`, `storage`, `review_queue`, `github`), so a use
  case moved into er-service before phase 6 would leave MCP behind.

## Decisions

Each question has a recommended default, and planning proceeds on the
defaults until the owner answers.

1. **A third track beside phase 5.** ADDENDUM-06 caps phase 5 at "two tracks
   at most, plus a C-only entry". 5b would be a third track. Default: allow
   it. Lane T shares no file with D, DS, U, US or P. 5b runs as one chain,
   one build at a time, and every heavy check takes the machine-wide lock. A
   C-only entry waits while three tracks run. Alternative: start 5b when the
   sidebar track ends (after P55.3).
2. **The TUI reads the published views in-process.** Default: the TUI holds
   each actor's `Arc<WorkspaceView>` and the registry's `TabsView`, and
   renders `TabContent`'s sections directly. It sends er-service `Command`s
   and `AppCommand`s. It uses no er-protocol type, so lane P is untouched and
   nothing is serialized on the TUI's hot path. Alternative: read er-protocol
   resources as the desktop does, which adds a copy per frame and ties the
   TUI to the desktop's wire.
3. **Where the actor host lives.** Default: a new er-service module, so the
   TUI and phase 6's MCP share one host. The desktop keeps its `ServiceState`
   until phase 6. Alternative: a host in er-tui, copied from the desktop's.
4. **The TUI's tab list.** Default: the TUI uses an `AppRegistry` in a new
   in-memory mode that reads and writes neither `tabs.json` nor
   `pr-open-cache.json`. The TUI's tabs come from its arguments today, and a
   TUI must never rewrite the desktop's saved tabs. The registry still issues
   every key and lists every tab, so P54's refusal holds. Alternative: share
   the open cache file with the desktop and accept last-writer-wins on a
   cache.
5. **A dual path behind two seams, and an incomplete flag-on TUI.** Default:
   first make every renderer draw from one read model (P63) and every key
   produce an action without reading the App (P64), with the flag-off TUI
   unchanged. Then the flag-on path fills the read model from the service and
   runs the actions as commands, area by area. As D21 allowed for the desktop,
   the flag-on TUI may lack an area until the entry that ports it, and
   `test-v1` covers each ported area. Alternative: port in place with the
   service always on, which leaves the App and the actor writing one bucket
   for the whole phase.
6. **TUI review submission is built on the service path only.** Default: P68
   builds it over the existing pending-review commands, so it reaches users
   when P76 removes the flag. If P76 has not merged at 1.0.0, the release
   notes keep D3's line. Alternative: build it on the App path first, which
   ships it earlier and is deleted in P76.
7. **Code that leaves the App.** Default: TUI-only view code (the view types,
   the hub builders, the composer and the overlays) is written into er-tui;
   the App keeps its copy until P77 deletes it. A domain step that a phase-5
   entry already extracts is reused, and the 5b entry waits for that entry.
   5b writes no second copy of a domain step. Alternative: copy a domain step
   into a new always-compiled file with a parity test against the App's
   method, so a 5b entry need not wait; ADDENDUM-05 B1 rejected lasting
   duplication of use cases.
8. **Git working-tree writes are domain writes.** Stage, unstage, stage all,
   commit, push, the watched snapshot, the watched-file delete and context
   expansion change the repo or the published diff. Default: each becomes an
   er-service command whose effect runs the `Git` port off the actor and
   refreshes the tab (P69). Alternative: keep them as TUI calls to the port,
   which the exit criterion forbids.
9. **The use cases move into er-service in phase 6, with MCP.** ADDENDUM-06
   A11 kept them in the engine "until 5b". After P77 the only callers outside
   er-service are er-mcp's direct calls, which phase 6 moves onto the service.
   Default: move them in phase 6, so MCP and the use cases move together.
   Alternative: move them in 5b and give er-mcp an er-service dependency
   early.
10. **5b deletes the App and the `ui` feature.** After P62.2 and P76 nothing
    enables `ui`. Default: P77 deletes `er_engine::app`, the `ui`-gated
    export wrapper and the feature, with the re-exports P58.2 and P61 left
    for the TUI. Alternative: leave the dead half to phase 6.
11. **The TUI's one-action AI pick.** The TUI's AI hub can pick a provider and
    model for the next action only (`pending_ai_selection_override`,
    `app/state/mod.rs:4729`, set at `:5247`, cleared after each run at
    `input/mod.rs:1014`). The owner dropped the desktop's session pick (A18),
    which is a different thing. Default: keep the one-action pick as
    `TuiView` state and pass it with the job, if P60.2's job path takes a
    selection; otherwise every pick saves, as on the desktop.

## Lane T and the files 5b shares with phase 5

- **Lane T** is `crates/er-tui/**`, its manifest included. No phase-5 entry
  holds it: P62.2 keeps the TUI flag and edits no er-tui file.
- **Lanes D, DS, U, US and P:** no 5b entry touches them.
- **Lane S, additions only.** 5b's er-service edits run beside the workspace
  track, the way ADDENDUM-06's pair (b) shares `ResourceKey` and the host
  dispatch:
  - a new host module and its `lib.rs` line (P65). No phase-5 entry
    touches the new file.
  - `protocol.rs` and `actor.rs`: new `Command` variants and their match
    arms (P69, P71's context lines, and P72 if a clear needs its own
    command), in their own section of the enum, with effects in a new
    module. P56.1 (the refresh route), P56.2
    (re-compact), P58.2 (eight commands), P59 (the stack refresh) and P60.2
    (`SubmitAiReply`, `AppendFindingResponse`) add variants to the same
    enum. Each side adds; the rebase is mechanical.
- **Lane S, waits.**
  - `app_registry.rs`: P54 edits it (labels, kinds, the launch root, the
    last-tab rule). P65's in-memory mode depends on P54.
  - `content.rs` and `effect.rs`: P71's context lines add a field to the
    diff section and a refetch to the load path. P56.1, P56.2 and P56.3 edit
    both files, so P71 depends on all three.
  - `crates/er-service/Cargo.toml`: P56.1 adds `test-support`. A 5b test that
    needs `Command::Stall` waits for P56.1; P65's no-wait test holds an
    effect on a gated fake port instead, so it does not wait.
- **The engine's `ui` half.** P56.3, P58.2 and P60.2 edit
  `app/state/mod.rs`, and P61 edits `export.rs`. P72 moves the four clear
  helpers after P58.2 has moved their neighbours. P77 deletes the half after
  P62.2, which follows every one of them.
- **Lane C.** P76 edits the `test-v1` recipe, the CI `test-v1` job, the lint
  gate's feature pass and the standalone check recipe, all of which P62.2
  edits first. P76 depends on P62.2. P77 edits er-engine's features.

## PR entries

Shared acceptance: `fmt-check` and `clippy` always; `test` (er-tui is in it);
`test-v1` for every entry with flag-on code; `test-all` and
`build-service-headless` when er-service changes; `docs-check` for docs and
ADRs. Every heavy check runs through
`~/.claude/scripts/locked.sh <worktree> just <recipe>`, one at a time. Until
P76, every entry leaves the flag-off TUI unchanged: its existing tests pass
untouched, and an entry that must change one says so up front. On the flag
path a key press never waits on an actor: commands go out with `send`, reply
receivers are drained each tick, and the loop thread never calls
`send_and_wait`. The stop line is 1,500 added-plus-changed lines after rename
detection; pure deletions do not count (ADDENDUM-04's L5). Each done-when
item is proven by a planted break, except doc items: those are proven by
`docs-check`, and the release-notes line by the reviewer's read.

Two test shapes recur. **Render parity**: the same fixture repo is rendered on
both paths to ratatui's `TestBackend`, and the buffers must match. **Key
parity**: the same key sequence runs on both paths, and the view state and the
sidecars on disk must match.

### P63 The render seam

- Branch: `epic/p63-tui-render-seam`. Lanes: T. Model: `opus`. Depends on:
  none.
- **Goal:** every renderer draws from one read model that either path can
  fill.
- **Done when:** `ui::draw` takes a `Screen`: a borrowed read model of the
  active tab's files, AI state, reviewed marks, PR data, history and tour,
  the tab's view fields, and the app chrome (tabs, status line, theme, update
  notice, config error); one function builds it from the App; no module under
  `ui/` names `App` or `TabState`; the view types the renderers read
  (overlays, hubs, input mode, confirm actions, split side, the config edit
  state) are er-tui types, converted from the App's in that one function; the
  watched snapshot diff (`ui/diff_view.rs:2880`) is read when the watched list
  is refreshed, and the renderer draws the result; a golden suite renders
  fixtures for each panel, each hub group, each diff mode (History and Tour
  included), the split diff and a watched file to `TestBackend`, with the
  goldens committed in the PR's first commit, before the refactor; the
  flag-off TUI is unchanged.
- **Planted breaks:** a conversion that drops `focused_finding_id` changes a
  golden and fails; a `use er_engine::app` under `ui/` fails the source scan;
  a renderer that calls a port fails the same scan.
- **Fallback split:** P63.1 the view types, overlays, hubs, settings and
  status bar; P63.2 the diff view, file tree and panel.
- Size: L.

### P64 The key seam

- Branch: `epic/p64-tui-key-seam`. Lanes: T. Model: `opus`. Depends on: P63.
- **Goal:** keys become actions without reading the App, so both paths share
  one key table.
- **Done when:** each handler in `input/` splits into a pure step from a key
  and the `Screen` to a `TuiAction`, and an executor; the flag-off executor
  calls today's App methods in today's order; the key table reads only the
  `Screen` and er-tui's view types; the direct writes in `input/mod.rs`
  (`:952`, `:972`, `:1039`, and the clears at `:965`–`:1028`) become
  `TuiAction` variants, so every domain write the TUI makes is named in one
  enum; a key-table test pins the action for every key in every input mode;
  the existing key tests (`main.rs`'s and `input/`'s) pass unchanged.
- **Planted breaks:** swapping two keys' actions fails the key-table test; a
  key step that takes `&App` fails the source scan of the key module; an
  executor arm that drops `Quit` fails `ctrl_q_sets_should_quit`.
- **Fallback split:** P64.1 normal mode and the overlays; P64.2 the comment,
  confirm, filter, commit, remote-URL and base inputs.
- Size: L.

### P65 The service host, the in-memory registry and the flag-on shell

- Branch: `epic/p65-tui-service-host`. Lanes: T, S (a new module; the
  registry edit after P54). Model: `opus`. Depends on: P64, P54
  (`app_registry.rs`).
- **Goal:** with the flag on, `er` opens its targets on the service, and
  renders and navigates them from the published views.
- **Done when:**
  - er-service gains a host module: one actor per workspace key, the settings
    actor and one registry, each started on first use, in the shape of the
    desktop's `ServiceState` without its desktop relays (decision 3);
  - `AppRegistry` gains an in-memory mode that never reads or writes
    `tabs.json` or `pr-open-cache.json` (decision 4);
  - with the flag on, `main.rs` turns its arguments (paths, `--pr`,
    `--remote`, PR URLs, `--filter`) into registry tabs and sends `LoadTab`
    or `OpenTarget`; a flag with no service route yet (`--target`, until
    P71) reports that on the status line;
  - the loop holds a `TuiView`: per tab key the selection, cursor, scroll,
    panel, layer toggles, Gate, filter, layout widths and text areas, and app
    wide the input mode and the overlay; each frame fills P63's `Screen` from
    the active tab's published content and the `TuiView`;
  - file, hunk and line navigation, scrolling and the jump to the next
    unreviewed file run on `TuiView` through `er_engine::navigation`, the
    functions A10's fixture holds to desktop-ui's copy;
  - lazy files parse through `ParseFiles` when the selection reaches a stub;
  - events from each actor and the registry wake the loop for a redraw;
  - each second the loop sends `ReloadSidecars` for every loaded key, which
    replaces the sidecar poll (`main.rs:579`) on the flag path;
  - `SessionState` is saved from the `TuiView` and the content's diff hash
    and restored on the same hash (A15); drafts restore whole, client id
    included (ADR 0053);
  - an ADR records the TUI's read path (decision 2), the in-memory registry
    and `TuiView`; ADR 0068 gains a consequence for the TUI's view state;
  - the flag-on startup and navigation work under render parity for the diff,
    the file tree and the status bar.
- **Planted breaks:** two tabs on one repo that start two actors fail the
  one-actor test; an argument opened outside the registry is refused by P54's
  unlisted-key check and fails the startup test, as does a `--target` that is
  silently ignored; a host that builds the registry with `AppRegistry::new`
  opens a seeded `tabs.json` and fails the no-persistence test, and a tab
  command that saves fails it too; a key handler that calls `send_and_wait`
  while an effect is held on a gated fake port misses the frame bound and
  fails the no-wait test; navigation done on an App fails the flag-on source
  scan (no `er_engine::app` in the flag-on modules); a `Screen` filled without
  the reviewed marks fails render parity; a restore that ignores the diff hash
  restores a selection on a changed diff and fails; a lazy stub shown without
  `ParseFiles` renders empty hunks and fails render parity; a loop that
  redraws on a timer only leaves the frame stale after a `ViewPublished` and
  fails the wake test; with the reload tick off, an outside write to
  `questions.json` leaves the AI section's revision unmoved and fails the
  reload test; a restored draft that drops its client id posts as a new
  comment and fails the draft test (ADR 0053); the ADR and the ADR 0068
  consequence are checked by `docs-check`.
- **Fallback split (the plan):** P65.1 the host and the in-memory registry
  (lane S); P65.2 the flag-on shell, `TuiView` and the session file.
- Size: L.

### P66 Settings, the config hub and the AI picker

- Branch: `epic/p66-tui-settings`. Lanes: T. Model: `opus`. Depends on: P65.
- **Goal:** the config hub and the AI selection go through the settings
  actor.
- **Done when:** the config hub reads the settings actor's view and writes
  through `SettingsCommand` (`ApplyConfigPatch`, `SetDefaultAiSelection`,
  `SetDefaultAiEffort`, the provider and model edits); model refresh is
  `RefreshAiModels`, which replaces the loop's discovery thread
  (`main.rs:631`–`:648`) on the flag path; the theme and the config load
  error come from the settings view; a settings warning shows once as a
  Notice on the status line; the one-action AI pick follows decision 11.
- **Planted breaks:** a hub edit that writes the App's config leaves the
  settings actor's revision unmoved and fails the one-writer test (ADR 0069);
  a model refresh on a TUI thread fails the source scan; a warning shown twice
  fails the Notice test; a theme or config-load error read from the App's
  config leaves the frame on the old theme after a settings change, and the
  banner absent after a failed load, and fails render parity.
- Size: M.

### P67 Questions, notes, threads and the composer

- Branch: `epic/p67-tui-threads`. Lanes: T. Model: `opus`. Depends on: P65.
- **Goal:** the TUI's own annotations are written by the actor.
- **Done when:** the composer, reply, edit, delete, resolve and promote-to-note
  keys send `AddQuestion`, `AddNote`, `ReplyToThread`, `UpdateThreadMessage`,
  `DeleteThread`, `ResolveThread` and `PromoteToNote`; a finding reply sends
  `ReplyToFinding` with `ai_assist` false (the AI-assist reply is P74's); each
  write carries the `TabContext` of the tab it was composed on; the composer
  text lives in `TuiView` and stays until the reply, so a refused write keeps
  it; each create carries a `client_id` (ADR 0017); the layer toggles, hide
  resolved and the Gate are `TuiView` state.
- **Planted breaks:** a composer that closes before the reply loses its text
  on a refused write and fails; a create with no `client_id` gets a fresh id
  and fails the confirm test; a write resolved from the active tab at reply
  time, after a tab switch, lands on the other tab and fails the keyed-write
  test; key parity on add, reply and delete fails when `questions.json`
  differs; a layer toggle, hide-resolved or Gate change written to the App or
  sent to the service fails the flag-on source scan (ADDENDUM-05 risk 6), and
  one that is dropped fails render parity.
- Size: M.

### P68 GitHub comments, sync, approval and review submission

- Branch: `epic/p68-tui-github`. Lanes: T. Model: `opus`. Depends on: P67.
- **Goal:** every GitHub write from the TUI goes through the actor, and the
  TUI can submit a review with a decision again (D3).
- **Done when:**
  - the GitHub composer and its replies send `AddComment` and
    `ReplyToGithubThread`; promote sends `PromoteToComment` and
    `PromoteFindingToComment`; "pull comments" sends `SyncGithubComments`;
    "comment on PR" sends `PostGithubPrComment`; approve sends
    `SubmitGithubPrDecision` with Approve, replacing the direct
    `gh_pr_approve` (`input/mod.rs:1039`);
  - review submission (D3, ADR 0049): the composer can add a line comment to
    the pending review (`AddPendingReviewComment`); a review hub reads the
    pending review (`GetPendingReview`), lists its comments, removes one
    (`RemovePendingReviewComment`) and submits it with Comment, Approve or
    Request changes and a summary (`SubmitPendingReview`);
    `PendingReviewChanged` refreshes the hub; a refusal, such as approving
    your own PR, shows its message;
  - a tab known to have no PR refuses the GitHub composer (ADR 0053); the
    startup PR probe stays a read on its worker thread and records present,
    absent or unknown in `TuiView`;
- **Planted breaks:** with a fake GitHub that refuses, a promote that removes
  its source fails the promote test (ADR 0053), and a sync that does not move
  the AI section's revision fails the sync test; Request changes sent as
  Comment fails the event test; a submit that skips `GetPendingReview` and posts a bare
  decision while a draft exists fails the draft test; a remove sent with the
  wrong node id fails; an approve through `gh_pr_approve` fails the source
  scan; an own-PR approval that shows no message fails the refusal test; a
  GitHub composer that opens on a tab known to have no PR fails; a hub that
  ignores `PendingReviewChanged` still lists a removed comment and fails the
  refresh test; a probe that records a failed lookup as absent refuses the
  composer on a tab whose PR is unknown and fails the probe test.
- **Fallback split:** P68.1 comments, sync and approval; P68.2 review
  submission.
- Size: L.

### P69 Working-tree commands

- Branch: `epic/p69-tui-worktree-commands`. Lanes: T, S (additions). Model:
  `opus`. Depends on: P65.
- **Goal:** the TUI's git writes run as actor effects (decision 8).
- **Done when:** er-service gains commands for stage, unstage, stage all,
  commit, push, the watched snapshot and the watched-file delete, each with
  its effect in a new module that runs the `Git` port off the actor and then
  refreshes the tab, so the diff section's revision moves; the watched list
  and its snapshot diffs are read on a TUI worker thread through er-git's
  read functions, and the TUI writes none of them; a refused push or commit
  shows the port's message. Context lines are P71's, because they change the
  diff section.
- **Planted breaks:** a commit run on the actor thread trips the port guard
  (ADR 0067) and fails; a stage whose effect skips the refresh leaves the
  diff section's revision unmoved and fails; a TUI call to the port's
  `stage_file`, `commit` or `push` fails the source scan; a watched-file
  delete for a path outside the repo root is refused, and dropping that check
  deletes the file and fails; a push refused by a fake port that leaves the
  status line blank fails the refusal test.
- **Fallback split:** P69.1 stage, unstage, commit and push; P69.2 watched
  files.
- Size: L.

### P70 Reviewed marks and the filter

- Branch: `epic/p70-tui-reviewed`. Lanes: T. Model: `opus`. Depends on: P65.
- **Goal:** reviewed marks are written by the actor, and the filter runs in
  the TUI.
- **Done when:** the reviewed keys send `ToggleReviewed` and `MarkReviewed`
  with the tab's `TabContext`; the reviewed count comes from the reviewed
  section; the filter, its history and the unreviewed-only toggle are
  `TuiView` state over the pure `visible_files`, as the desktop's filter is
  after P56.1; compaction expands through `ParseFiles` with `expand`
  (re-compacting is P71's).
- **Planted breaks:** a reviewed write after a tab switch that resolves the
  active tab lands on the other tab and fails the keyed-write test; a
  reviewed count computed in the TUI from marks it holds fails the source
  scan; a filter read from an App tab fails the same scan; key parity on
  mark, filter and jump fails when `reviewed.json` differs; an expand that
  sends `ParseFiles` without `expand` leaves the file compacted and fails
  render parity.
- Size: M.

### P71 Diff modes, the base, the watcher and compaction

- Branch: `epic/p71-tui-modes-watcher`. Lanes: T, S (`content.rs` and
  `effect.rs` for context lines, after P56.1–P56.3 have merged their edits
  there; additions to `protocol.rs` and `actor.rs`). Model: `opus`. Depends
  on: P56.1 (the refresh route with auto-unmark, the base route, the checkout
  root, the mtime order), P56.2 (re-compact), P56.3 (History, Conflicts and
  Hidden), P69 (staging), P70.
- **Goal:** every diff mode, the base change and the file watcher run on the
  service.
- **Done when:** the mode keys send `SetDiffMode` for every mode;
  History's commit list, commit selection and file list come from P56.3's
  section and commands; Conflicts and Hidden load in the workspace; the base
  input and `--target` use the route P56.1 gives `change_base`; the watcher
  sends P56.1's refresh route with auto-unmark for the active key instead of
  `refresh_diff_quick_with_unmark` (`main.rs:559`), and the status line
  shows how many reviewed marks it cleared; the mtime sort reads the order
  P56.1 publishes; a compacted file re-compacts through P56.2's command;
  staging in Conflicts sends P69's command; context expansion gets a service
  command that sets one file's context-line count, which the diff section
  keeps per path across loads, as `DiffSection::expanded` keeps an expanded
  file, and which the load path refetches with
  (`git::refetch_file_with_context`).
- **Planted breaks:** a watcher that refreshes nothing leaves the diff
  section's revision unmoved after a file write and fails; a cleared-mark
  count that is not shown fails the notice test; a History view filled from
  the mode the tab left fails render parity; a base change that only edits
  `TuiView` leaves the diff unchanged and fails; a re-compact that leaves the
  path expanded fails the toggle test in the second direction; a Conflicts or
  Hidden view filled from the App fails the flag-on source scan; a file order
  that ignores the published mtime order fails render parity with the sort
  on; a Conflicts stage that leaves the conflict listed after the refresh
  fails the resolve test; a context-line count lost on reload fails the
  reload test.
- **Fallback split:** P71.1 the watcher, the base and the mtime sort (after
  P56.1); P71.2 History, Conflicts, Hidden and re-compact (after P56.2 and
  P56.3); P71.3 context lines (lane S).
- Size: L.

### P72 Findings, checklist, tour and the clears

- Branch: `epic/p72-tui-findings-tour`. Lanes: T, S (the engine's
  always-compiled half; additions to `protocol.rs` and `actor.rs` if a clear
  needs its own command). Model: `opus`. Depends on: P58.2 (the checklist
  command, `DeleteReviewArtifact`, and the moved cleanup helpers), P71.
- **Goal:** the review panel, the tour and the clear actions write through the
  actor.
- **Done when:** the checklist toggle sends `ToggleChecklistItem`; the tour's
  reviewed toggle and pillar bulk review send `ToggleReviewed` and
  `BulkReviewPillar`; tour, finding and panel navigation are `TuiView` state;
  the four clear helpers (`app/state/mod.rs:8454`, `:8490`, `:8505`,
  `:8522`) move to the always-compiled half beside the ones P58.2 moved, stay
  re-exported for the flag-off path, and run as actor effects: clearing a
  review through `DeleteReviewArtifact` where it deletes the same files, and
  through a new command for questions and notes, question answers and note
  replies; each clear passes the bucket admission check that guards a review
  flow's lease.
- **Planted breaks:** a checklist toggle written to the App fails the source
  scan, as does a finding or pillar jump done on the App; a clear on a bucket whose workflow lease is held is refused, and
  skipping the admission check deletes the review and fails; a pillar bulk
  review that misses one in-diff path fails key parity; a moved helper that
  changes what it deletes fails the flag-off clear test.
- Size: M.

### P73 The PR panel and the stack

- Branch: `epic/p73-tui-pr-stack`. Lanes: T. Model: `opus`. Depends on: P59
  (the stack in the PR section and its refresh command), P68.
- **Goal:** the PR overview and the stacked-PR rows come from the PR section.
- **Done when:** the PR panel reads the PR section's overview and commits; the
  stack rows read P59's stack and refresh through its command, which replaces
  the loop's lookup threads (`main.rs:617`–`:628`); opening a stack PR sends
  `OpenTarget`.
- **Planted breaks:** a stack refresh run on a TUI thread fails the source
  scan; a stack change that leaves the panel on the old rows fails render
  parity after the PR section's revision moves; a PR panel filled from the
  startup probe's overview fails render parity once a refresh moves the PR
  section; a stack PR opened by any route except `OpenTarget` leaves the
  registry without the tab and fails the open test.
- Size: S.

### P74 AI runs, review flows, the arena and the agent log

- Branch: `epic/p74-tui-ai-runs`. Lanes: T. Model: `opus`. Depends on: P60.1
  (arena and flows keyed through the registry, the descriptor-resolved
  bucket), P60.2 (agent runs on the service, the raw-diff step,
  `SubmitAiReply`, `AppendFindingResponse`), P72.
- **Goal:** every AI Hub action and named command runs as a job on the actor.
- **Done when:** every `spawn_agent_prompt` site the TUI reaches (review,
  validate, questions, notes, experts, professor, triage), the tour, the
  importance agent and the named `[commands]` run through P60.2's job path,
  each holding an Agent slot (ADR 0021); prompts are built from the content's
  AI and diff sections; the AI-assist finding reply runs as on the desktop;
  review flows send `Flow` commands and arena actions send `Arena` commands;
  the agent log panel and the command status rows are built by the TUI from
  `WorkspaceView` and the job events; cancel sends `CancelJob`; on the flag
  path `check_commands`, `poll_background_tasks` and `drain_agent_log`
  (`main.rs:585`–`:591`) no longer run.
- **Planted breaks:** a run spawned from a TUI thread fails the source scan,
  as do an arena or flow command resolved from an App tab and a flag-on loop
  that still calls `check_commands`; a
  progress line that does not reach the log panel fails the log test; a
  cancel that does not reach the runner fails the reused `CancelJob`
  assertion (P43); an answer that lands on the active tab after a tab switch
  fails the captured-bucket test; a one-action pick that is not cleared after
  its run fails the pick test; with the slot cap at one, a second run starts
  before the first ends and fails the slot test (ADR 0021); a prompt built
  from an App tab fails the source scan, and a questions prompt that misses a
  question added on the flag path fails the prompt test; an AI-assist reply
  that skips the in-flight entry shows no pending reply and fails.
- **Fallback split:** P74.1 agent runs, named commands and the log; P74.2
  review flows, the arena, importance and the tour.
- Size: L.

### P75 Export, copy and open actions

- Branch: `epic/p75-tui-export-copy`. Lanes: T. Model: `opus`. Depends on: P61
  (the export renderer in the always-compiled half), P74.
- **Goal:** the read-only actions read the content.
- **Done when:** the export picker renders through P61's renderer over the
  content; the copy actions (context, review JSON, questions JSON, hunk,
  line, path, full file) read the content; open-in-editor and open-in-browser
  go through `os_launch` with the path and line from `TuiView`.
- **Planted breaks:** an export that renders an App tab fails the source scan;
  an export missing a question added on the flag path fails the export test;
  a copied hunk read from the App fails the same scan, and one that copies the
  previous file's hunk after a selection change fails the copy test; an
  open-in-editor that sends the App's line fails the launch test, which
  records the path and line given to `os_launch`.
- Size: S.

### P76 Switch the TUI and delete its old path and the flag

- Branch: `epic/p76-tui-switch`. Lanes: T, C. Model: `opus`. Depends on:
  P63–P75, P62.2 (the same recipe, CI and lint-gate lines).
- **Goal:** the TUI runs only on the service.
- **Done when:** the flag-off executor, the App-filled `Screen` and the old
  loop are deleted; er-tui depends on er-service as a plain dependency and on
  er-engine with `default-features = false` and the features it uses, its
  dev-dependency included, so no build of er-tui turns `ui` on; the
  `v1-service` feature leaves er-tui, and with it the `test-v1` recipe, the
  CI `test-v1` job and the lint gate's feature pass, which cover only er-tui
  after P62.2; `cargo check -p er-tui` joins the standalone check P62.2 adds
  for er-desktop, because a workspace build unifies er-engine's features; a
  guard test in er-tui fails on any call to a store write, a `Git` port write,
  an er-github write, or a `std::fs` write outside `update.rs`; the release
  notes carry D3's restored review submission; `crates/er-tui`'s docs and
  `CLAUDE.md` describe the TUI on the service.
- **Planted breaks:** a `use er_engine::app::App` in er-tui fails the
  standalone check locally and in CI; a `std::fs::remove_file` added to a key
  action fails the guard test; a test that reads er-tui's manifest fails when
  a `v1-service` feature is re-added; the docs are checked by `docs-check`.
- Size: M (deletions do not count).

### P77 Delete the App and the `ui` feature

- Branch: `epic/p77-delete-app`. Lanes: S, C. Model: `opus`. Depends on: P76.
- **Goal:** the engine has one half.
- **Done when:** `crates/er-engine/src/app`, the `ui`-gated export wrapper and
  the `ui` feature are deleted, with the re-exports P58.2 and P61 kept for the
  TUI; the use cases stay in the always-compiled half (decision 9); ADR 0006
  is marked superseded in full, ADR 0068's D20 sentence and ADR 0067's
  crate-graph consequence describe the one-half engine; every doc that cites a
  deleted file is updated, and `docs-check` passes.
- **Planted breaks:** a reference to `er_engine::app` anywhere fails the
  build; a test that reads er-engine's manifest fails when a `ui` feature is
  re-added; a doc left citing `app/state/mod.rs` fails `docs-check`.
- Size: M (deletions do not count).

## The chain

- **Starts now:** P63. It holds lane T only and depends on nothing.
- **Then, on earlier 5b work:** P64 after P63; P65 after P64 and P54;
  P66, P67, P69 and P70 after P65; P68 after P67. Past P54, none of these
  waits on a phase-5 entry.
- **Waits on phase 5:** P71 (P56.1, P56.2, P56.3), P72 (P58.2), P73 (P59),
  P74 (P60.1, P60.2), P75 (P61), P76 (P62.2), and P77 after P76.
- Lane T runs one entry at a time, in this order: P63 → P64 → P65 → P66 →
  P67 → P68 → P69 → P70 → P71 → P72 → P73 → P74 → P75 → P76 → P77. An entry
  whose phase-5 dependency has not merged gives its turn to the next ready
  one; the Depends-on lines, which become each issue's Blocked-by line, carry
  every code dependency.
- With decision 1's default, three tracks run at once: the workspace track,
  the sidebar track until P55.3, and lane T.

## What 5b asks of phase 5's plan

- **The track ceiling** (decision 1). ADDENDUM-06's "two tracks at most, plus
  a C-only entry" becomes three tracks, and a C-only entry waits while they
  run.
- **P54's unlisted-key refusal** must read the registry's published tab list,
  which the TUI's in-memory registry fills. It must not read `tabs.json`.
  Its tests should build the registry without assuming it persists.
- **P56.1's refresh route with auto-unmark** should answer how many reviewed
  marks it cleared, since the TUI's status line shows that count
  (`main.rs:560`–`:566`). If P56.1 merges without it, P71 adds the field, as
  an addition.
- **P58.2 and P61's re-exports** from `er_engine::app` serve only the flag-off
  TUI from P62.2 on; P77 removes them. No change to either entry.
- **`test-v1` and the lint gate's feature pass** run er-tui's flag-on code in
  every phase-5 PR from P65 on, so a 5b break can turn a phase-5 PR red. The
  coordinator treats a red `test-v1` on a phase-5 PR as a 5b fix.
- **A1's reason for builders in er-service** no longer includes the TUI
  (decision 2). The builders stay where A1 put them, for phase 6's MCP.

## Risks

1. **Two render paths until P76.** Render and key parity are the guard. The
   coordinator stops new flag-on work if a flag-off regression lands.
2. **The read model is a wide seam.** `Screen` carries many fields, and a
   field the old path fills but the flag path leaves empty shows only in
   parity. Each flag-on entry adds parity fixtures for the area it ports.
3. **A blocked loop freezes the terminal.** One `send_and_wait` on the loop
   thread can hold a frame for 30 s (`REPLY_TIMEOUT`). P65's no-wait test
   and the source scan guard it.
4. **Two processes, one config.** The TUI's settings actor and the desktop's
   each save `config.toml`, as both processes do today. 5b does not change
   this; ADR 0005's rule against a reload that overwrites live config holds.
5. **The TUI waits on most of phase 5.** Eight of fifteen entries depend on
   a phase-5 entry, so lane T will sit idle between them. This is expected and
   costs no rework.
6. **v0.6.x fixes land on the App path** (D2). The coordinator ports any TUI
   fix that a gate merge brings into the flag-on executor of the entry that
   owns the area.
7. **Machine load.** 5b adds one build at a time to phase 5's two tracks; the
   lock keeps heavy checks serial.

## ADR and doc changes

- P65 adds an ADR for the TUI's read path, the in-memory registry and
  `TuiView`, and a consequence to ADR 0068.
- P76 updates `CLAUDE.md` and the TUI's docs.
- P77 marks ADR 0006 superseded in full and updates ADRs 0067 and 0068.
- No `CONTEXT.md` term changes. `TuiView` and `Screen` are code names.
