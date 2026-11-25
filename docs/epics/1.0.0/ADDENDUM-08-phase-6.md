# Addendum 08: phase 6, consolidate

This is the phase-6 plan, drafted for the owner's approval. No phase-6 issue
opens and no phase-6 build starts until the owner approves it (decision 1).
It follows [PLAN.md "Re-planning at a phase
gate"](./PLAN.md#re-planning-at-a-phase-gate). Once approved, where this file
disagrees with PLAN.md or an issue body, this file wins.
[ADDENDUM-01](./ADDENDUM-01-decisions.md),
[ADDENDUM-02](./ADDENDUM-02-issue-decisions.md),
[ADDENDUM-05](./ADDENDUM-05-phase-4.md),
[ADDENDUM-06](./ADDENDUM-06-phase-5.md) and
[ADDENDUM-07](./ADDENDUM-07-phase-5b.md) still bind.

Sources, all on #400: the owner's gate decisions
([issuecomment-7162983329](https://github.com/VilfredSikker/easy-review/issues/400#issuecomment-7162983329),
"gate" below), the planner's citation check and step plan
([issuecomment-7163627907](https://github.com/VilfredSikker/easy-review/issues/400#issuecomment-7163627907),
"planner"), and the owner's answers to the planner's questions with the
coordinator's rulings
([issuecomment-7165887831](https://github.com/VilfredSikker/easy-review/issues/400#issuecomment-7165887831),
"answers"). Where the answers and the planner differ, the answers win.

Every citation below was re-checked by symbol against `release/v1.0.0` at
`15b0d1d6e` (2026-11-25). The planner read `64d94bc5d`; between the two,
the `main` merge (#766) changed only `.claude/` and `.gitignore`, so no
crate moved. Symbols are the anchor; this file gives no line numbers.

PLAN's phase-6 milestone:

- Goal: inbox and projects in the service; MCP on the service; CLI
  subcommands; superseding ADRs.
- Exit criteria: front ends depend only on `er-protocol` and `er-service`,
  checked by an `er-lint-gate` rule over `cargo metadata` (no `deny.toml`);
  one `now_epoch_ms`; `CONTEXT.md` changed only where a term changed.
- Known risk: MCP behaviour changes visible to installed skills.
- Open question: which CLI subcommands ship in 1.0.0. Answered by decision 2.

## Hand-offs from earlier addenda

- **ADDENDUM-07 decision 3**: the desktop keeps its `ServiceState` until
  phase 6. It still does (`crates/er-desktop/src/service.rs`, beside
  er-service's `ServiceHost`). Owner: P84.
- **ADDENDUM-07 decision 9**: the use cases move into er-service with MCP.
  Owner: P80.1, recorded as done in P81.1's ADR.
- **ADDENDUM-06 A13**: the sidebar owner moves without touching desktop-ui.
  The `SidebarOwner` trait the resource host reads is in place. Owner: P79.
- **ADR 0070**: names phase 6's inbox and project records as following
  `AppRegistry`'s home. Owner: P79.1, which adds the consequence.
- **PLAN, "Verified starting point"**: dependency-direction rules are left
  to phase 6. Owner: P83.3.
- **Phase-5 exit list (BUILD_LOG)**: the sidebar half of `route_helpers.rs`
  and `page_view.rs` leaves in P79.2 and P79.3, and the rest of that split
  stays a follow-up; `DesktopState.branch_base_remote_oid` and the last
  comment citing `commands.rs` go in P83.2; the `*Snapshot` renames are
  P86; `has_producers` is #764. `TabState::adopt_key` is already gone, and
  `docs/github-sync-architecture.md` and `docs/mobile-port.md` are already
  fixed. `status_fetch_clock`, the `diff(tab)` row cache (#612) and the
  click-to-paint numbers (#616) stay follow-ups.
- **Phase-5b exit list (BUILD_LOG)**: #762, the refusal dwell and the
  release-note lines (#765) are built alongside this phase (decision 3). The
  `compile_fail,E0277` gap stays a toolchain follow-up. The write guard's
  match-arm gap stays a follow-up; P83.1 narrows it, since er-tui can then name
  no er-engine, er-git or er-store writer.

## Claims checked against the tree

Method: `git grep` and file reads at `15b0d1d6e`. The planner's §1 holds
except where noted.

- **The sidebar owner is desktop code.** er-desktop's `projects.rs`,
  `inbox.rs`, `pr_cache.rs`, `gh_status_cache.rs`, `auto_triage.rs`,
  `app_owner.rs` (with `app_owner/tests.rs`) and `app_owner/startup.rs`
  hold the records and the owner. `AppOwner`, `SidebarOwner`,
  `build_projects`, `build_branches`, `build_prs`, `build_inbox`,
  `worktree_rows`, `write_inbox` and `set_wake` are in `app_owner.rs`.
  The resource host reads the trait (`resources/host.rs` `sidebar()`,
  `resources/pr.rs`, `gh_pr_actions.rs`). The sidebar types
  (`BranchInfo`, `PrInfo`, `GithubStatusSnapshot`, `RepoMergeSnapshot`,
  `WorktreeSnapshot`, `GhUser`, `GhStatusCache`, `ProjectMeta`,
  `MetaCache`) and the refreshers (`refresh_meta_cache`,
  `refresh_meta_cache_for_project`, `build_worktrees`) are in
  `page_view.rs`. The status fetch, the PR fetch, the inbox sweep, the
  notifications, the triage and the project commands are in
  `route_helpers.rs` (`fetch_and_store_github_status` …
  `service_set_active_project`).
- **Correction: auto-triage's entry points.** `auto_triage.rs` exposes
  `dispatch_auto_triage` and `dispatch_branch_triage`; `run_pr_triage` and
  `run_branch_triage` are the Tauri command names that reach them.
- **Three writers of the projects record.** er-desktop `projects.rs`
  `update_file` (under `PROJECTS_IO`, through `projects_pins::update`),
  er-mcp `pr_saved` (`projects_pins::pin_pr` and `unpin_pr`), and
  er-engine `first_launch` (`import_legacy_projects_once`).
- **The desktop registers three store kinds itself**
  (`store_registry.rs` `register_desktop_kinds`: `gh-status-cache`,
  `inbox`, `pr-cache`). `the_shared_composition_does_not_claim_desktop_kinds`
  and `the_mcp_registry_does_not_claim_desktop_kinds` pin that no other
  composition claims them.
- **er-mcp depends on er-engine alone**, through `server.rs`, `projects.rs`
  and `main.rs`. The modules only er-mcp calls are `pr_resolve`,
  `pr_review_feedback`, `review_queue`, `sidecar_summary` and
  `diagram_upload`; no other crate or engine module names them.
  `sidecar_upload` has engine callers (`job_commit.rs`, `diagram_upload.rs`,
  `sidecar_summary.rs`) and stays in the engine. `gh_pr_list_queue`,
  `gh_pr_checks_state_remote`, `gh_pr_thread_addressing_remote`,
  `gh_pr_title` and `default_handle()` reach `gh` outside `Ports`.
- **Two writers of one write.** `pr_review_feedback`'s
  `reply_to_question_in_dir`, `reply_to_note_in_dir` (author defaults to
  `"agent"`, through `append_question_reply`), `resolve_question_in_dir`
  and `resolve_note_in_dir` (answering `already_resolved`) duplicate
  er-service's `annotations::reply_to_thread` and `resolve_thread`. The
  service's reply goes through `add_private`, which fetches the diff; MCP's
  copies the parent's anchor and fetches nothing. The finding reply is
  already shared (`append_finding_response`).
- **Correction: er-mcp has 11 tests**, five in `server.rs`, four in
  `projects.rs` and two in `tests/registry.rs`. None is a golden of a
  tool's schema or output. The server registers 13 tools.
- **Front-end manifests.** er-tui's `[dependencies]` name er-agent, er-engine
  (`watch`, `highlight`), er-git, er-service, er-store and er-protocol; its
  only er-agent use is `er_agent::agent_run::real_cli_refusals` in
  `service/ai_runs_tests.rs`. er-desktop's name er-engine (`watch`),
  er-store, er-service and er-protocol, and it has a
  `[target.'cfg(target_os = "macos")'.dependencies]` table. er-mcp's name
  er-engine. `crates/er-tui/tests/manifest.rs`
  `every_engine_dependency_leaves_default_features_off` asserts two
  er-engine entries.
- **No dependency rule reads `cargo metadata`.** er-lint-gate's nearest
  precedent is `core_io.rs` `ALLOW_LISTS`, which reads `[dependencies]`
  only. There is no `deny.toml`.
- **Clock helpers.** Milliseconds: `now_epoch_ms` in er-desktop
  `inbox.rs` (public), `pr_cache.rs` and `projects.rs` and in er-engine
  `projects_pins.rs`; `now_ms` in er-desktop `route_helpers.rs` and
  er-service `tasks.rs`. Seconds: `now_epoch_secs` in er-desktop
  `gh_status_cache.rs`, er-engine `github_post.rs` and
  `github_post_session.rs`, er-mcp `server.rs` (an `i64`) and er-store
  `store.rs`; `unix_now` in er-engine `model_discovery.rs` and
  `release_update.rs`. Out of scope: the formatters (`chrono_now`,
  `iso_now`, `unix_now_secs_string`, `chrono_like_timestamp`), the
  monotonic counters and the version clocks.
- **The headless pieces exist.** `ServiceHost` (`host.rs`) has `flow`,
  `run` and `with_events`; `FlowRequest::Start` and `RunStandard` are in
  `run_worker.rs`; `FlowState::AwaitingConfirmation` is in the engine's
  `ai/review_flow.rs`; `WorkspaceView::review_flow` reads a bucket's flow;
  `Event::WorkflowChanged` reports it. The TUI's start steps (`plan`,
  `local_pr`, `open`, `load`) are in `crates/er-tui/src/service/startup.rs`.
  `PrFindingItem` is in `pr_review_feedback.rs`. `ER_STORAGE_ROOT`
  (`paths.rs`, `storage.rs`) and `ER_CONFIG_PATH` (`config/mod.rs`) are
  honoured at runtime.
- **The pending-review path exists.** `Command::GetPendingReview`,
  `AddPendingReviewComment` and `SubmitPendingReview` (with
  `ReviewEvent::Comment`) run in `pending_review.rs`. `submit` refuses the
  viewer's own PR for `Approve` only. `ai::promote::promote_finding` turns
  a finding into a comment and refuses one with no line
  (`PromoteError::NoLine`).
- **Correction: the `*Snapshot` renames reach desktop-ui.** desktop-ui
  names `AppSnapshot` on 306 lines in 92 files (the planner counted about
  60). `WatchStatusSnapshot`, `ReviewFlowSnapshot` and
  `FailedReviewFlowFinderSnapshot` take 74 lines, one of them a doc comment
  in `crates/er-protocol/src/jobs.rs` that the generated TypeScript copies.
  `GithubStatusSnapshot`, `RepoMergeSnapshot` and `WorktreeSnapshot` take
  113 lines, 23 of the files in desktop-ui, so renaming them in P79.1, as the
  planner proposed, would edit desktop-ui. All six renames go to P86. The
  `AppSnapshot` strings in `crates/er-lint-gate/src/command_shape.rs` are
  test inputs of the `Snapshot` suffix rule and stay.
- **Correction: #764's branch holds more of lane S.** Its first commit edits
  `actor.rs`, `host.rs`, `runs.rs`, `flows.rs`, `tasks.rs` and the flow
  driver tests. The planner named `flows.rs` and `tasks.rs` only.

## Decisions

Owner decisions, as given; each names its source.

1. **The gate runs now; the owner approves this addendum before any phase-6
   issue opens or builds** (gate 1).
2. **The CLI in 1.0.0 is `er review --pr N`**, headless, with JSON output
   and an optional `--post`, on the headless service path that MCP on the
   service builds. No other subcommand ships in 1.0.0 (gate 2).
3. **#762, #764 and #765 are built alongside the gate and land before
   1.0.0** (gate 3). This plan holds them as dependencies only: P81.1 and P84
   wait for #764, and P83.1 waits for #762 and #765. #760 and #761 go with
   the release; both are already fixed there.
4. **"`just ci` passes through the lock" is covered by green CI on the PR
   that merges `main` into the release** (gate 4). For #766 that CI ran the
   change detector and the docs check and skipped the code jobs, since no
   code changed; the last full run is P77.2's (#763), green. Local checks stay
   cheap, as in phases 5 and 5b.
5. **All of phase 6 ships in 1.0.0** (answers 1), P78 to P86 with decision
   14's entry.
6. **"Depend only on er-protocol and er-service" is a manifest rule**
   (answers 2). er-service re-exports the engine types the front ends use and
   wraps their local effects. An er-lint-gate rule over `cargo metadata`
   checks the manifests; dev-dependencies are exempt, and er-mcp counts as a
   front end. Type coupling stays: ADDENDUM-07 decision 2 keeps `DiffFile`,
   `AiState` and `PrOverviewData` on `TabContent`. What the rule buys is
   that a new effectful engine call from a front end has to pass through a
   reviewed er-service function.
7. **The whole sidebar owner moves into the service** (answers 3): the
   records, both caches, auto-triage and the refreshers.
8. **One er-service `headless` module** serves MCP and `er review` (answers
   4). MCP calls its functions and starts no actor; `er review` adds a
   `ServiceHost` for its run.
9. **`er review` runs the default review flow** (ADR 0044) (answers 5). A
   triage that picks Deep is lowered to Standard, and the report says so
   (`depth_lowered: true`). A headless run never confirms an arena's cost.
10. **`--post` posts one pending GitHub review, submitted as `COMMENT`**
    (answers 6; ADR 0049). It sends one notification. A rerun after a failure
    refuses on the leftover draft, so nothing is posted twice. Posted findings
    stay in the bucket. This replaces the planner's promote path, and P81.2 is
    planned on it.
11. **Refs and storage** (answers 7): the TUI's `--pr` open, which fetches
    the PR head and base refs, and the managed root, with `ER_STORAGE_ROOT`
    and `ER_CONFIG_PATH` for CI.
12. **npm** (answers 8): `easy-review-mcp` and `@easy-review/skills` are
    republished at 1.0.0, with the v1 storage path, after the MCP contract is
    frozen. P85 prepares both packages; the publish runs with the release.
13. **P84, one service host, is in phase 6** (answers 9).
14. **The renames are P86, the last entry** (answers 10): desktop-ui's
    `AppSnapshot` page type and the three Rust `*Snapshot` types that do
    not move. The three that move with the sidebar are renamed there too
    (see the correction above).
15. **The clock lives in `er_core::clock`** (answers, coordinator ruling).
    `er::core_io` allows it, and er-github gains an er-core dependency.
16. **er-tui's er-agent dependency moves to dev-dependencies in P83.1**, once
    a build confirms no production use (answers, coordinator ruling).

Taken from the planner into this plan; the owner's approval of this addendum
approves them:

17. **The MCP stability policy** (planner §3). No tool is renamed or removed;
    arguments and output keys are only added, and an added argument is
    optional. `diff-tmp` on disk and the `bucket_path` and `diff_tmp_path`
    keys are part of the contract. A breaking change ships as a new tool name
    beside the old one for a release. er-mcp's server version follows the
    workspace version and becomes 1.0.0 with the release; the skills keep
    their own `metadata.version`.
18. **`er review` exit codes** (planner §4): 0 when the flow ends `Done` or
    triage skips; 1 on `Failed`, `Cancelled`, `Interrupted`, or a load or
    open failure; 2 on a usage error; 3 when the review is done and `--post`
    refused or failed, with the report still printed and `post.error` set.

## Open question for the owner

- **A lost submit response.** Decision 10 relies on the leftover draft to
  stop a second post. One case leaves no draft: GitHub accepts
  `SubmitPendingReview` and the response is lost (a timeout, a dropped
  connection). Inside one process P81.2 reads the pending review back after
  an unknown outcome and reports the review as posted when the draft is gone.
  A new process has nothing to read back, so a rerun posts a second review.
  **Recommended:** the review body ends with a hidden marker of the bucket
  and `diff_hash`, and `--post` refuses when the viewer already has a
  submitted review on the PR carrying the same marker. Alternative: accept
  the gap and say so in the release notes. P81.2 builds neither until this
  is answered; the rest of P81.2 does not wait for it.

A rerun after a success is a new review: it runs the flow again and, with
`--post`, posts a new review. That follows from decision 10 (findings stay
in the bucket) and is stated in the release notes.

## Lanes

- **M** `crates/er-mcp/**`, the `npm/er-mcp` package and `npm/skills`.
- **S** `crates/er-service/**`. Until #764 merges, its branch holds
  `actor.rs`, `host.rs`, `runs.rs`, `flows.rs`, `tasks.rs` and the
  flow driver tests; an entry that edits them waits for #764.
- **E** `crates/er-engine/**`.
- **D** and **DS** as in ADDENDUM-06: DS is `projects.rs`, `pr_cache.rs`,
  `inbox.rs`, `gh_status_cache.rs`, `auto_triage.rs` and the `app_owner`
  module, plus, for this phase, the sidebar functions of `route_helpers.rs`
  and `page_view.rs` and the sidebar tests under `route_helpers/tests/`.
  D is the rest of `crates/er-desktop/src/**`.
- **T** `crates/er-tui/**`. Until #762 and #765 merge, their builder holds
  `service/shell.rs`, `service/loop_tests.rs`, `service/watched_tests.rs`,
  `service/config_hub_tests.rs`, the settle helpers in `service/tests.rs`
  and `docs/release-notes/tui-on-the-service.md`.
- **P** `crates/er-protocol/**` and the generated TypeScript (regenerated
  with `just protocol-gen`, never hand-merged).
- **U** `desktop-ui/src/**`. Only P86 takes it.
- **C** root `Cargo.toml` and `Cargo.lock`, `clippy.toml`, `justfile`,
  `.github/workflows/ci.yml`, `crates/er-lint-gate/**` and every front-end
  manifest.
- **Docs** `docs/adr/**`, `CLAUDE.md`, `docs/DEVELOPMENT.md`, release
  notes.

One entry holds a lane at a time. Two entries that share a lane run together
only where the chain below names it, with additions on both sides.

## PR entries

Shared acceptance, as in phases 5 and 5b: local checks stay cheap
(`fmt-check`, `docs-check`, `comments-check`, `cargo check --workspace
--all-targets`, `check-desktop` when er-desktop or engine features change,
`clippy` once per PR), each through the machine-wide lock; GitHub CI runs
the full suites, and the coordinator merges on green CI. Each done-when item
is proven by a planted break that fails, run locally on the one test
concerned, except doc items, which `docs-check` and the reviewer's read
prove. The stop line is 1,500 counted lines (insertions of
`git diff --shortstat -M`); a whole-file move counts as a rename and counts
its edits only, while functions cut out of a larger file count in full. From
P78's merge on, every PR reproduces P78's goldens byte for byte; a
re-record is a reviewed change named up front in the PR body.

### P78 Freeze the MCP contract

- Branch: `epic/p78-mcp-contract`. Lanes: M. Model: `opus`. Depends on:
  nothing; starts on approval.
- **Goal:** every behaviour an installed skill can see is pinned before
  anything moves.
- **Done when:** a golden of `tools/list` (the 13 names, descriptions and
  input schemas) is checked in; each store-backed tool has an output golden
  over a temp storage root and `Ports::for_test` (`projects_list`,
  `pr_saved`, `pr_summarize`, `pr_upload` accepted and rejected,
  `pr_prepare`, `pr_guide`, `pr_diagram`, and `pr_feedback_get`,
  `pr_feedback_reply` and `pr_feedback_resolve` in both buckets), with
  temp-root paths, generated ids and timestamps masked as P76.1's goldens
  masked theirs, so P82's clock change cannot move them; the tools that reach
  `gh` outside `Ports` (`prs_query`, `pr_stats`, the `pr_resolve` forms
  that call `default_handle()`) are listed in the PR with one captured
  real-`gh` output each, and their goldens land in P80.1; er-mcp's README
  states decision 17's policy.
- **Tests that change:** none. The 11 existing tests stay as they are.
- **Planted breaks:** renaming a tool, renaming an argument, dropping an
  output key, changing a reply's default author and changing a tool
  description each fail a golden; a temp path left unmasked fails the golden
  on a second temp root.
- **Size:** about 1,100 to 1,400, mostly fixtures. **Fallback split:** P78a
  `tools/list` and the store-backed tools; P78b the rest. Take it if the
  first build passes 1,300.

### P79 The app records in er-service

Goal of the three PRs: inbox, projects, the PR cache, the gh-status cache,
auto-triage and the refreshers live in er-service under one app-scoped owner
(ADR 0070, decision 7). The desktop keeps the Tauri relay, the native
notification display, window code and `app_owner/startup.rs`.

#### P79.1 Move the records and the owner

- Branch: `epic/p79-1-app-records`. Lanes: S, DS, D. Model: `opus`.
  Depends on: nothing; runs beside P78.
- **Done when:** `projects.rs`, `inbox.rs`, `pr_cache.rs`,
  `gh_status_cache.rs`, `app_owner.rs` and `app_owner/tests.rs` move into
  an er-service app-records module, with the `page_view` types they need,
  under their current names (P86 renames); `app_owner/startup.rs` stays in
  er-desktop, since it reads `DesktopState` and `route_helpers`; the
  `profile_log` calls in `pr_cache.rs` become a change hook the desktop
  passes in, beside `set_wake`; `SidebarOwner` is an er-service trait, and
  the resource host and desktop-ui are unchanged; the three store kinds
  register through an er-service function that only the desktop's
  composition calls; every write of the projects record in a process goes
  through the owner; ADR 0070 gains a consequence (the inbox and projects
  followed its home).
- **Tests that change:** the desktop tests that import the moved modules
  (`sidebar_tests.rs`, `sidebar_inbox_tests.rs`, `sidebar_pr_tests.rs`,
  `pr_resource_tests.rs`, `sync_command_tests.rs`, `tab_resource_tests.rs`,
  `tab_key_tests.rs`, `workspace_key_tests.rs` and `task_tests.rs` under
  `route_helpers/tests/`) change their imports only; `app_owner/tests.rs`
  moves with its assertions; the two "does not claim" tests keep theirs.
- **Planted breaks:** a TUI or MCP composition that registers `inbox`
  fails the existing "does not claim" tests; a desktop module that writes
  `inbox.json` or `projects.json` without the owner fails a source scan; a
  PR-cache refresh that skips the change hook leaves the desktop revision
  unbumped and fails the hook test; a moved `SidebarOwner` read that loses a
  version stamp fails the existing app-owner tests.
- **Size:** about 700 (whole-file moves count their edits; the cut
  `page_view` types count in full). **Fallback split:** none needed at the
  estimate.

#### P79.2 The sidebar refreshers

- Branch: `epic/p79-2-sidebar-refresh`. Lanes: S, DS, D. Model: `opus`.
  Depends on: P79.1.
- **Done when:** the metadata refresh (`refresh_meta_cache`,
  `refresh_meta_cache_for_project`, `build_worktrees`), the PR-cache
  refresh and the remote PR fetch (`fetch_single_pr_for_remote`,
  `cache_single_pr_for_remote`, `kick_pr_list_refresh`) and the GitHub
  status fetch (`fetch_github_status`, `fetch_and_store_github_status`,
  `kick_github_status_refresh`) run in er-service, called by the desktop's
  loops; `DesktopState` holds the owner and no cache maps.
- **Tests that change:** the refresher tests under `route_helpers/tests/`
  move with their code, assertions kept.
- **Planted breaks:** a refresh that writes a cache without publishing
  through the owner fails the owner's version test; a status fetch run on the
  caller's thread fails the existing no-wait scans.
- **Size:** about 1,100, counted in full because the functions are cut out
  of `route_helpers.rs` and `page_view.rs`. **Fallback split:** P79.2a the
  metadata refresh and worktrees; P79.2b the PR cache and the status fetch.

#### P79.3 The inbox sweep, the triage and the project commands

- Branch: `epic/p79-3-inbox-projects`. Lanes: S, DS, D. Model: `opus`.
  Depends on: P79.2.
- **Done when:** `process_inbox_after_pr_refresh`,
  `ingest_github_notifications`, `file_in_inbox` and
  `file_tasks_in_inbox`, the inbox open targets
  (`resolve_inbox_open_target` … `service_open_inbox_target`), the project
  and branch commands (`track_branch` … `service_set_active_project`) and
  `auto_triage.rs` (`dispatch_auto_triage`, `dispatch_branch_triage`,
  planned on er-service's task host with no task-host file edited) run in
  er-service; the desktop keeps `show_native_notification` and
  `maybe_send_native_notification` and asks the service what to notify.
- **Tests that change:** the sidebar test files under `route_helpers/tests/`
  move with their code, assertions kept.
- **Planted breaks:** an inbox write that skips `save_inbox_state` fails the
  inbox reload test; a triage planned outside the task host fails the
  existing scan; a notification shown twice for one item fails the seen-set
  test.
- **Size:** about 1,300. **Fallback split:** P79.3a the inbox sweep and
  notifications; P79.3b the project and branch commands and auto-triage.

### P80 MCP on the service

#### P80.1 The MCP use cases in er-service, one writer

- Branch: `epic/p80-1-mcp-use-cases`. Lanes: S, E, M. Model: `opus`.
  Depends on: P78.
- **Done when:** `pr_resolve`, `pr_review_feedback`, `review_queue`,
  `sidecar_summary` and `diagram_upload` move whole from er-engine into
  er-service's `headless` module (decision 8), which also holds bucket
  resolution and the feedback reader with its finding rows; the free `gh`
  calls MCP makes go through `Ports.github`, and P78's deferred goldens are
  recorded; question and note replies and resolves have one writer in
  er-service, which MCP and the `ReplyToThread` and `ResolveThread`
  effects both call; every P78 golden is unchanged.
- **Main risk:** the two reply writers differ (see the claims). The plan is
  one writer that builds a reply from its parent's row with no fetch, used by
  both, after a test shows the service's reply rows equal MCP's for an
  anchored parent. If they differ, the builder stops and the coordinator
  rules: keep the effect's fetch, or accept the row change in the release
  notes.
- **Tests that change:** `pr_review_feedback`'s inline tests move with the
  module, assertions kept.
- **Planted breaks:** a second writer of a question reply fails a source scan
  that allows one; the author default changed to `"You"` fails the reply
  golden; an MCP reply that fetches a diff fails the scripted-port test (no
  `gh` call expected); a `gh` call outside the port fails the scripted-port
  test of `prs_query`.
- **Size:** about 700 (the moves are renames). **Fallback split:** P80.1a
  the module moves, the port routing and the deferred goldens; P80.1b the
  writer merge.

#### P80.2 er-mcp depends on er-service

- Branch: `epic/p80-2-mcp-on-service`. Lanes: M, C. Model: `sonnet`.
  Depends on: P80.1, P79.1 (`projects_list` and `pr_saved` go through the
  projects owner).
- **Done when:** `crates/er-mcp/Cargo.toml` `[dependencies]` names
  er-service and er-protocol as its only workspace crates, and every engine
  item er-mcp names (`Ports`, `git::ProdDiffStats`, `storage`, the store
  audit, the `sidecar_upload` calls) comes through an er-service re-export or
  a `headless` function; MCP starts no actor; `projects.rs` reads and
  `pr_saved` writes through the owner; the README's "Thin REST API over
  `er-engine`" line is corrected; every golden is unchanged.
- **Tests that change:** `crates/er-mcp/tests/registry.rs` reads the
  composition through er-service; its assertions stay. `projects.rs`'s
  tests keep er-engine's `with_test_storage_root` through the
  dev-dependency.
- **Planted breaks:** er-engine re-added to er-mcp's `[dependencies]` fails
  a manifest test (P83.3's rule later); a `pr_saved` pin that writes
  `projects.json` itself fails the owner's one-writer scan.
- **Size:** about 500. **Fallback split:** none, well under the line.

### P81 Headless `er review --pr N`

#### P81.1 The run and the JSON report

- Branch: `epic/p81-1-headless-review`. Lanes: S, T, P. Model: `opus`.
  Depends on: P80.1 (the finding rows), #764 (`flows.rs` and `host.rs`,
  the code this run waits on).
- **Done when:** `er_service::headless::review` builds a `ServiceHost` with
  the in-memory registry (no `tabs.json` or `pr-open-cache.json` read or
  written), opens the PR as the TUI's `--pr` does (decision 11), waits for
  the load, sends `FlowRequest::Start`, follows `Event::WorkflowChanged`,
  answers `FlowState::AwaitingConfirmation` with `FlowRequest::RunStandard`
  (decision 9), waits for a terminal state bounded by the configured agent
  timeouts, and returns a versioned er-protocol `HeadlessReview`
  (`schema`, the PR and its head oid, `bucket_path`, `diff_hash`, the flow's
  depth, `depth_lowered`, state, kept and dropped, the findings in
  `PrFindingItem`'s row shape, the summary, and an empty `post`); the
  TUI's start steps (`local_pr`, `open`, `load`) move into er-service and
  both callers use them; a third `Commands` variant in
  `crates/er-tui/src/main.rs` parses `er review --pr <N> [--post]`, prints
  the report as the only stdout, enters no raw mode, installs no TUI panic
  hook, and exits with decision 18's codes; the run starts no file watcher,
  model discovery, update check, first-launch notice, view file, inbox or
  cache; tests run on the gated fake agent (er-service `test-support`) and
  the scripted GitHub port; the PR shows that a run on a fresh temp root and
  config writes no config file; a new ADR records the headless path (MCP
  and the CLI call one er-service module; a headless run never confirms
  Deep) and records ADDENDUM-07 decision 9 as done.
- **Tests that change:** the TUI startup tests follow the start steps into
  er-service, assertions kept.
- **Planted breaks:** a stray `println!` in the run fails the stdout test
  (the report must parse as the only output); a Deep plan confirmed
  headlessly fails the depth test; `Interrupted` mapped to 0 fails the
  exit-code test; a run that writes `tabs.json` fails the temp-root scan; a
  renamed report key fails the report golden.
- **Size:** about 1,150. **Fallback split:** P81.1a the er-service run and
  report; P81.1b the CLI and the exit codes.

#### P81.2 `--post`, one pending review

- Branch: `epic/p81-2-headless-post`. Lanes: S, T. Model: `opus`. Depends
  on: P81.1.
- **Goal:** `er review --pr N --post` posts the run's findings as one
  pending review submitted with `COMMENT` (decision 10, ADR 0049).
- **Done when:**
  - with `--post`, the run reads the viewer's pending review
    (`GetPendingReview`) before the flow starts; a pending review, the
    viewer's own draft or a draft an earlier run left, refuses the post
    with exit 3 and no agent run, and `post.error` names the draft and how
    to clear it on GitHub;
  - after the flow, the run reads the pending review again before its first
    add, since a draft can appear during the run, and refuses the same way;
  - each kept finding with a line in the diff goes into the pending review
    through `AddPendingReviewComment`, its path, line, side and body taken
    from `ai::promote::promote_finding`; a finding with no line, or with a
    line outside the diff, goes into the review body below the summary;
  - the review is submitted with `SubmitPendingReview` and
    `ReviewEvent::Comment` only, never `Approve` or `RequestChanges`.
    `refuse_own_pr` gates `Approve` alone, so a comment review on the
    viewer's own PR posts;
  - no finding leaves the bucket;
  - a failed add stops the post with no submit: the draft stays on GitHub,
    the report prints with `post.error` and exit 3, and a rerun refuses on
    the draft;
  - after a submit with an unknown outcome, the run reads the pending review
    back: none means the review was posted, and the report says so; a draft
    still there is submitted once more;
  - `post` in the report carries the inline count, the in-body count and
    the outcome;
  - the PR records one real run on a scratch PR (PLAN's gh-writes risk) and
    one fake-backed run of each path above.
- **Tests that change:** none.
- **Planted breaks:** a post while a pending review exists fails the refusal
  test, and so does a refusal checked only before the flow (the fake port
  adds a draft during the run); a submit with any event other than
  `COMMENT` fails the fake port's assertion; a finding body built outside
  `ai::promote` fails a source scan of the headless module; a finding
  removed from the bucket after the post fails the bucket test; a no-line
  finding missing from the body fails the body test; a submit after a failed
  add fails the partial-failure test; a rerun after a partial failure that
  posts anything fails the rerun test; an unknown submit outcome that
  resubmits when the draft is gone fails the read-back test.
- **Size:** about 550. **Fallback split:** none, well under the line.
- The marker in the open question above is not part of this entry until the
  owner answers.

### P82 One `now_epoch_ms`

- Branch: `epic/p82-one-clock`. Lanes: C, E, S, D, M, and er-store and
  er-github. Model: `sonnet`. Depends on: P79.3, P80.2 (the desktop and MCP
  helpers have moved), #764 (`tasks.rs`).
- **Done when:** `er_core::clock::{now_epoch_ms, now_epoch_secs}` exist
  (decision 15); every millisecond and seconds helper in the claims is
  deleted and its callers use the two, er-mcp's `i64` caller converting at
  its one site; er-github gains an er-core dependency; the formatters, the
  monotonic counters and the version clocks (`status_fetch_clock` included)
  are untouched and named in the PR as out of scope.
- **Tests that change:** a test that called a deleted helper calls
  `er_core::clock`; no assertion changes.
- **Planted breaks:** a second `fn now_epoch_ms` anywhere in `crates/`
  fails a source scan; a new `fn now_epoch_secs` outside er-core fails it
  too.
- **Size:** about 350. **Fallback split:** none, well under the line.

### P83 Front ends depend on er-service and er-protocol only

#### P83.1 The er-service front-end surface, and the TUI on it

- Branch: `epic/p83-1-tui-deps`. Lanes: S, T, C. Model: `opus`. Depends
  on: #762 and #765 (`service/shell.rs`), P81.1 (`main.rs`), P82.
- **Done when:** er-service re-exports the engine data types and pure
  helpers the front ends name and wraps the front-end-local effects
  (highlight, watcher, launch, copy, export, dir browse, update, uninstall,
  first launch, the view file) (decision 6); er-service gains `watch` and
  `highlight` features, off by default, that turn on er-engine's, so
  `build-service-headless` still checks the bare graph and `check-desktop`
  still checks each front end in its own graph; the TUI's watched-file reads
  go through an er-service function; er-tui's `[dependencies]` lose
  er-engine, er-git and er-store, and er-agent moves to dev-dependencies once
  a build confirms no production use (decision 16).
- **Tests that change, up front:** `crates/er-tui/tests/manifest.rs`
  `every_engine_dependency_leaves_default_features_off` asserts one
  er-engine entry (the dev-dependency) where it asserts two today.
  `crates/er-tui/tests/writes.rs` keeps its four allowances (the session
  save, `update.rs`, `first_launch::notice` in `service/shell.rs`,
  `uninstall::execute` in `main.rs`); where a call now goes through an
  er-service wrapper, its needle follows the wrapper's name in the same file.
- **Planted breaks:** a `use er_engine::` left in er-tui's `src` fails the
  build; dropping the `highlight` feature fails the highlight test; an
  er-service default feature list that names `watch` fails a manifest test
  and `build-service-headless`; a write called through a wrapper outside the
  allowed file fails `writes.rs`.
- **Size:** about 600. **Fallback split:** P83.1a the er-service surface;
  P83.1b the TUI's imports and manifest.

#### P83.2 The desktop on it

- Branch: `epic/p83-2-desktop-deps`. Lanes: S, D, C. Model: `sonnet`.
  Depends on: P79.3, P83.1, P84 (both hold D; P84 deletes `ServiceState`,
  whose imports this entry would otherwise rewrite).
- **Done when:** er-desktop's `[dependencies]` and its macOS target table
  lose er-engine and er-store (dev-dependencies keep them); the dead
  `DesktopState.branch_base_remote_oid` goes with its write in
  `route_helpers.rs` `align_branch_base_probe`; the comment in
  `crates/er-engine/src/pr_open_cache.rs` that cites `evict_lru` "in
  commands.rs" is corrected.
- **Tests that change, up front:** `route_helpers/tests/content_route_tests.rs`
  drops its read of `branch_base_remote_oid`, which goes with the field; the
  probe test keeps its other assertions.
- **Planted breaks:** a `use er_store::` left in er-desktop's `src` fails
  the build; `check-desktop` still passes.
- **Size:** about 500. **Fallback split:** none at the estimate.

#### P83.3 The `er::front_end_deps` rule

- Branch: `epic/p83-3-front-end-deps`. Lanes: C. Model: `sonnet`. Depends
  on: P80.2, P83.1, P83.2.
- **Done when:** er-lint-gate runs `cargo metadata --format-version 1
  --no-deps` and fails when er-tui, er-desktop or er-mcp declare a normal or
  build dependency on a workspace crate other than er-service and
  er-protocol, target-specific tables and renamed packages included;
  dev-dependencies are exempt; the rule lands at error tier with zero
  findings and no budget (D7's alternative, as P09 and P12 did).
- **Tests that change:** none.
- **Planted breaks:** er-engine under er-tui's `[dependencies]` fails;
  er-store under er-desktop's macOS target table fails; er-git under a
  renamed key (`git = { package = "er-git", … }`) fails; er-engine under
  `[dev-dependencies]` passes.
- **Size:** about 450. **Fallback split:** none, well under the line.

### P84 One service host

- Branch: `epic/p84-one-host`. Lanes: S, D. Model: `opus`. Depends on:
  P79.3, #764 (`host.rs`).
- **Goal:** the desktop runs on er-service's `ServiceHost`, and
  `ServiceState` is deleted, closing ADDENDUM-07 decision 3 (decision 13).
- **Done when:** `ServiceHost` takes a persisting `AppRegistry` and the
  app-records owner from its builder, while the TUI and the headless run keep
  the in-memory registry and no records; the desktop's revision relay and
  resource wake hang off `with_events`; the `load_requests` dedupe moves
  with it; every `ServiceState` test passes against the host.
- **Tests that change, up front:** the six tests in `service.rs` change
  their setup to build a `ServiceHost`; assertions kept.
- **Planted breaks:** a host built for the TUI that persists `tabs.json`
  fails the existing in-memory test; a desktop host that drops the revision
  relay fails the relay test; the `load_requests` dedupe gains a test in the
  move, and with the dedupe removed a second request inside its window starts
  a second load and fails it.
- **Size:** about 700. **Fallback split:** P84a the host builder options;
  P84b the desktop switch and the deletion.

### P85 ADRs, docs, npm and the close-out

- Branch: `epic/p85-docs`. Lanes: Docs, M (the npm packages). Model:
  `sonnet`. Depends on: P78 to P84.
- **Done when:** ADR 0067's crate-graph section states the front-end rule,
  or a new ADR does and ADR 0067 cites it; ADR 0002 gains a line on the
  threaded `ServiceHost` the TUI runs (ADR 0073); docs-check passes both ways
  on every superseded mark; `CLAUDE.md` and `docs/DEVELOPMENT.md` name
  `er review`, `ER_STORAGE_ROOT` and `ER_CONFIG_PATH`; the release notes
  cover `er review` (the JSON report, the exit codes, `--post` and the
  rerun rule), MCP's stability policy and the skills' storage path; the
  `easy-review-mcp` and `@easy-review/skills` packages are at 1.0.0 and
  the skills' sources and built copies name the v1 storage path (decision
  12); `CONTEXT.md` changes only if a term changed, and none is expected.
- **Tests that change:** none.
- **Planted breaks:** a superseded mark without its line in the superseding
  ADR fails docs-check; a skill that still names the old storage path fails a
  new test beside `npm/skills/scripts/sync-skills.test.js`.
- **Size:** about 400. **Fallback split:** none, well under the line.

### P86 The `*Snapshot` renames

- Branch: `epic/p86-snapshot-renames`. Lanes: U, D, P, S. Model: `sonnet`.
  Depends on: P79.1 (the moved types), P83.2 and P84 (lane D).
- **Goal:** no type or page type is named `*Snapshot` after the snapshot
  path is gone (decision 14).
- **Done when:** desktop-ui's `AppSnapshot` page type and every Rust
  `*Snapshot` type (`WatchStatusSnapshot`, `ReviewFlowSnapshot`,
  `FailedReviewFlowFinderSnapshot`, and the moved `GithubStatusSnapshot`,
  `RepoMergeSnapshot`, `WorktreeSnapshot`), with their desktop-ui mirrors,
  carry names from `CONTEXT.md`'s vocabulary, chosen on the issue; the
  er-protocol doc comment is updated and the TypeScript regenerated with
  `just protocol-gen`; the `command_shape` test inputs stay; no wire key or
  serialized field changes.
- **Tests that change:** tests that name a renamed type follow the rename;
  no assertion changes.
- **Planted breaks:** a type renamed in Rust with its desktop-ui mirror left
  behind fails `check-ui`; a changed serialized field fails the existing
  wire tests and `protocol-check`.
- **Size:** about 500 (306 lines for `AppSnapshot`, 74 for the three that
  stay, 113 for the three that moved). **Fallback split:** P86a desktop-ui's
  `AppSnapshot`; P86b the Rust types and their mirrors.

## The chain

- **Starts on approval:** P78 and P79.1, beside each other (M against S,
  DS, D).
- **MCP and headless track:** P78 → P80.1 → P80.2 (also after P79.1) →
  P81.1 (also after #764) → P81.2.
- **App-records track:** P79.1 → P79.2 → P79.3 → P84 (also after #764).
- **Closing sequence:** P82 (after P79.3, P80.2) → P83.1 (after #762, #765,
  P81.1) → P83.2 (after P84) → P83.3 → P85; P86 after P83.2 and P84, beside
  P83.3 and P85.
- **Lane S is shared.** P79.x, P80.1, P81.x and P84 all edit er-service, in
  new modules (app records, `headless`) and with additions to `lib.rs`.
  P79.1 and P80.1 may run together; whichever merges second merges the
  other in. P84 and P81.1 both edit `host.rs` and run one at a time, in
  order of readiness. P82 and P84 become ready together after P79.3; P82's
  edits are one-line call-site swaps, so they may run together, and
  whichever merges second merges the other in.
- **The Depends-on lines** become each issue's Blocked-by line and carry
  every code dependency.

Totals: 9 entries, 15 PRs (more if a fallback split is taken), about 10,900
counted lines. P78, P79.2, P79.3 and P81.1 sit nearest the stop line and name
their splits.

## What phase 6 asks of the follow-ups and the release

- **#764 lands before P81.1, P82 and P84.** Its branch edits `flows.rs`,
  `host.rs`, `actor.rs`, `runs.rs` and `tasks.rs`; the headless run
  waits on the flow those files drive.
- **#762 and #765 land before P83.1**, which rewrites er-tui's imports in
  `service/shell.rs` and its tests.
- **The release** publishes `easy-review-mcp` and `@easy-review/skills` at
  1.0.0 after P85 (decision 12).
- **A main merge before P83.3** keeps er-mcp's manifest and the lint gate in
  step with any v0.6.x fix that reaches them.

## Risks

1. **MCP behaviour that skills see** (PLAN's risk). P78's goldens hold it;
   P80.1's writer merge is the one planned change of behaviour, and it stops
   for a ruling if the rows differ.
2. **A second post after a lost submit response** (the open question). Until
   the owner answers, a rerun after a lost response can post a second
   review.
3. **A partly built draft.** A failed add leaves a pending review on GitHub
   that blocks the viewer's next review from the app as well as the rerun.
   The report says how to clear it; the app's Review popover shows it
   (ADR 0049).
4. **Three writers of `projects.json` across processes.** P79.1 gives each
   process one writer; across processes the store's scope lock still orders
   them, as today.
5. **A large sidebar move.** P79.2 and P79.3 cut functions out of
   `route_helpers.rs` and `page_view.rs` and count in full; each names its
   split.
6. **er-service grows a front-end surface.** Decision 6's re-exports widen
   er-service's public API. P83.3's rule keeps new engine calls from front
   ends passing through it; the reviewer reads each new re-export.
7. **Feature unification.** er-service's `watch` and `highlight` features
   could leak into the headless build through a workspace build.
   `build-service-headless` and `check-desktop` keep each graph checked on
   its own.
8. **Headless runs cost agent time.** `er review` runs real agents on the
   configured provider; CI jobs set `ER_CONFIG_PATH` to choose one, and the
   tests run only the gated fake agent.
9. **Machine load.** Two tracks run at once, plus a C-only entry; heavy
   checks take the lock.

## ADR and doc changes

- P79.1: a consequence on ADR 0070 (the inbox and projects followed its
  home).
- P81.1: a new ADR for the headless path; it records ADDENDUM-07 decision 9
  as done.
- P85: ADR 0067's crate-graph section (or a new ADR it cites) states the
  front-end rule; ADR 0002 gains the threaded-host line; `CLAUDE.md`,
  `docs/DEVELOPMENT.md`, the release notes and the npm packages.
- No `CONTEXT.md` term changes. The owner and module names are code names.
