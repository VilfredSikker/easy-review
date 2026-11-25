# Addendum 08: phase 6, consolidate

This is the phase-6 plan. The owner approved it on 2026-11-25 and then
revised its scope the same day: phase 6 adds no new features, the headless
CLI leaves for a post-1.0.0 epic, and every desktop page and route comes into
line with the service structure (decisions 2 and 14). This revision applies
that change; no phase-6 issue opens until it merges.
It follows [PLAN.md "Re-planning at a phase
gate"](./PLAN.md#re-planning-at-a-phase-gate). Where this file disagrees with
PLAN.md or an issue body, this file wins.
[ADDENDUM-01](./ADDENDUM-01-decisions.md),
[ADDENDUM-02](./ADDENDUM-02-issue-decisions.md),
[ADDENDUM-05](./ADDENDUM-05-phase-4.md),
[ADDENDUM-06](./ADDENDUM-06-phase-5.md) and
[ADDENDUM-07](./ADDENDUM-07-phase-5b.md) still bind.

Sources, all on #400: the owner's gate decisions
([issuecomment-7162983329](https://github.com/VilfredSikker/easy-review/issues/400#issuecomment-7162983329),
"gate" below), the planner's citation check and step plan
([issuecomment-7163627907](https://github.com/VilfredSikker/easy-review/issues/400#issuecomment-7163627907),
"planner"), the owner's answers to the planner's questions with the
coordinator's rulings
([issuecomment-7165887831](https://github.com/VilfredSikker/easy-review/issues/400#issuecomment-7165887831),
"answers"), and the owner's scope revision
([issuecomment-7166276265](https://github.com/VilfredSikker/easy-review/issues/400#issuecomment-7166276265),
"revision"). Later sources win over earlier ones, and all of them over the
planner.

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
- Open question: which CLI subcommands ship in 1.0.0. Answered by decision
  2: none.

## Hand-offs from earlier addenda

- **ADDENDUM-07 decision 3**: the desktop keeps its `ServiceState` until
  phase 6. It still does (`crates/er-desktop/src/service.rs`, beside
  er-service's `ServiceHost`). Owner: P84.
- **ADDENDUM-07 decision 9**: the use cases move into er-service with MCP.
  Owner: P80.1, recorded as done in its ADR.
- **ADDENDUM-06 A13**: the sidebar owner moves without touching desktop-ui.
  The `SidebarOwner` trait the resource host reads is in place. Owner: P79.
- **ADR 0070**: names phase 6's inbox and project records as following
  `AppRegistry`'s home. Owner: P79.1, which adds the consequence.
- **PLAN, "Verified starting point"**: dependency-direction rules are left
  to phase 6. Owner: P83.3.
- **Phase-5 exit list (BUILD_LOG)**, its structural items folded in by
  decision 14: the sidebar half of `route_helpers.rs` and `page_view.rs`
  leaves in P79.2 and P79.3, and P92 splits the rest by caller;
  `status_fetch_clock` is P91; `DesktopState.branch_base_remote_oid` and the
  last comment citing `commands.rs` go in P93; the `*Snapshot` renames are
  P86; `has_producers` is #764. `TabState::adopt_key` is already gone (no
  hit in `crates/`), and `docs/github-sync-architecture.md` and
  `docs/mobile-port.md` are already fixed. The `diff(tab)` row cache (#612)
  and the click-to-paint numbers (#616) are measurements, not structure, and
  stay follow-ups.
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
- **Correction: the `*Snapshot` renames reach desktop-ui.** desktop-ui
  names `AppSnapshot` on 306 lines in 92 files (the planner counted about
  60). `WatchStatusSnapshot`, `ReviewFlowSnapshot` and
  `FailedReviewFlowFinderSnapshot` take 74 lines, one of them a doc comment
  in `crates/er-protocol/src/jobs.rs` that the generated TypeScript copies.
  `GithubStatusSnapshot`, `RepoMergeSnapshot` and `WorktreeSnapshot` take
  113 lines, 23 of the files in desktop-ui, so renaming them in P79.1, as the
  planner proposed, would edit desktop-ui. All six renames go to P86.
  desktop-ui has 46 more `*Snapshot` interfaces (1,356 lines in 292 files),
  named for the deleted snapshot path; see the open question. The
  `AppSnapshot` strings in `crates/er-lint-gate/src/command_shape.rs` are
  test inputs of the `Snapshot` suffix rule and stay.
- **Correction: #764's branch holds more of lane S.** Its first commit edits
  `actor.rs`, `host.rs`, `runs.rs`, `flows.rs`, `tasks.rs` and the flow
  driver tests. The planner named `flows.rs` and `tasks.rs` only.

## Decisions

Owner decisions, as given; each names its source.

1. **The gate runs now; the owner approves this addendum before any phase-6
   issue opens or builds** (gate 1). Approved 2026-11-25, then revised
   (revision).
2. **Phase 6 adds no new features; the feature list stays as today's**
   (revision). The headless `er review --pr N` (gate 2), `--post`, its exit
   codes and its rerun marker move to a new post-1.0.0 CLI epic, which
   carries these as decisions already made: the default review flow with
   Deep lowered to Standard (answers 5); one pending review submitted as
   `COMMENT` (answers 6, ADR 0049); a hidden marker of the bucket and diff
   hash, checked before posting (revision); the TUI's `--pr` open, the
   managed root, and `ER_STORAGE_ROOT` and `ER_CONFIG_PATH` for CI (answers
   7). That epic reuses decision 8's `headless` module. No CLI subcommand
   ships in 1.0.0.
3. **#762, #764 and #765 are built alongside the gate and land before
   1.0.0** (gate 3). This plan holds them as dependencies only. #760 and
   #761 go with the release; both are already fixed there.
4. **"`just ci` passes through the lock" is covered by green CI on the PR
   that merges `main` into the release** (gate 4). For #766 that CI ran the
   change detector and the docs check and skipped the code jobs, since no
   code changed; the last full run is P77.2's (#763), green. Local checks stay
   cheap, as in phases 5 and 5b.
5. **Scope** (answers 1, as revised): the structural work, P78 to P86 without
   the CLI, and the page audit's entries, P87 to P93.
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
8. **One er-service `headless` module, as far as MCP needs it** (answers 4,
   revision). MCP calls its functions and starts no actor. It holds what MCP
   calls and nothing more.
9. **npm** (answers 8): `easy-review-mcp` and `@easy-review/skills` are
   republished at 1.0.0, with the v1 storage path, after the MCP contract is
   frozen. P85 prepares both packages; the publish runs with the release.
10. **P84, one service host, is in phase 6** (answers 9).
11. **The renames are P86** (answers 10): desktop-ui's `AppSnapshot` page
    type and the three Rust `*Snapshot` types that do not move. The three
    that move with the sidebar are renamed there too (see the claims).
12. **The clock lives in `er_core::clock`** (answers, coordinator ruling).
    `er::core_io` allows it, and er-github gains an er-core dependency.
13. **er-tui's er-agent dependency moves to dev-dependencies in P83.1**, once
    a build confirms no production use (answers, coordinator ruling).
14. **Every other desktop page and route comes into line with the service
    structure, as the sidebar is** (revision), with the phase-5 exit list's
    structural items. The page audit below lists what it found, and P87 to
    P93 carry it. Where an earlier choice kept something in er-desktop
    because only the desktop has it (ADR 0074's three runs, P58.2's browser
    annotation writes), this decision supersedes it, and the entry adds the
    consequence to the ADR.

Taken from the planner into this plan:

15. **The MCP stability policy** (planner §3). No tool is renamed or removed;
    arguments and output keys are only added, and an added argument is
    optional. `diff-tmp` on disk and the `bucket_path` and `diff_tmp_path`
    keys are part of the contract. A breaking change ships as a new tool name
    beside the old one for a release. er-mcp's server version follows the
    workspace version and becomes 1.0.0 with the release; the skills keep
    their own `metadata.version`.

## Open question for the owner

- **desktop-ui's other `*Snapshot` interfaces.** Beyond `AppSnapshot`,
  desktop-ui declares 46 interfaces and types named `*Snapshot`
  (`FileSnapshot`, `AiSnapshot`, `PrSnapshot`, `SettingsSnapshot` and the
  rest), on 1,356 lines in 292 files. They name the page's view of a
  resource, and the name comes from the deleted snapshot path. Decision 11
  names only `AppSnapshot`. **Recommended:** a follow-up after 1.0.0; the
  rename changes no behaviour and would take two or three more PRs.
  Alternative: add them to P86 as P86c onward.

## Page audit

Method: each desktop page and the routes behind it, read at `15b0d1d6e` for
direct engine or store calls from er-desktop, state in `DesktopState` that
belongs to a service owner, helpers in `route_helpers.rs` or `page_view.rs`
with one caller, and shapes left from the App. Of `route_helpers.rs`'s
helpers, every one outside the sidebar has exactly one calling module. The
sidebar and inbox are P79's and are left out.

- **Review page, diff and file tree** (`diff`, `hunks`, `reviewed`,
  `preview`, the content routes). Served from the actors' content. Left:
  `resources/preview.rs` reads files through `er_engine::preview_read`, and
  `direct/line_commits.rs` runs blame through the git port and keeps its
  cache in `DesktopState.line_commits_cache`. Both are reads; the cache is
  the desktop's own and stays. Owner: P83.2 routes the reads through
  er-service. The content routes' helpers in `route_helpers.rs` (one caller
  each) go in P92; `align_branch_base_probe` writes the dead
  `branch_base_remote_oid` and goes in P93.
- **Overview and the PR card** (`pr`, `pr_status`, the GitHub senders).
  Comments, line comments, decisions and the pending review go to the actor.
  The branch actions do not: `RunGithubPrAction` runs
  `er_engine::gh_pr_actions` from `crates/er-desktop/src/gh_pr_actions.rs`.
  Owner: P87. `status_fetch_clock` moves every PR's `pr_status` on any
  fetch. Owner: P91. `after_github_write` and `review_comment_body` have one
  caller. Owner: P92.
- **Right panel: threads, findings and card asks.** Annotation writes go to
  the actor. The card asks do not: `ai_ask.rs` plans and runs
  `run_card_ai_subprocess` on a desktop thread and keeps the pending replies
  in `DesktopState.pending_ai_replies`. Owner: P88.
- **Runs: lenses, tour, importance, diagram, scoped review, send note.**
  Lenses, tour and importance plan in er-service. The diagram, the scoped
  review and the send-note plan in `runs.rs`, which creates bucket folders
  with `std::fs` and prepares the diff with `ensure_diff_artifacts` itself.
  Owner: P89.
- **Arena and review flows** (`arena_service.rs`, `flow_service.rs`). Each
  sends commands to the actor and reads the published view: on the
  structure. `ReviewFlowSnapshot` and `FailedReviewFlowFinderSnapshot` sit
  in `page_view.rs`. Owners: P92 (place), P86 (name).
- **Browser view and its annotations.** The proxy and webview are window
  code and stay. The annotation writes do not follow the structure: the
  desktop writes `ui-annotations.json` and its screenshots itself, under
  `DesktopState.ui_annotations_io`, with its own `ui_annotations` clock.
  Owner: P90.
- **Jobs and the agent log** (`jobs`, `runs`, `task_hooks.rs`). The task
  host is er-service's; `DesktopState.tasks` holds it beside the host the
  TUI uses. Owner: P84.
- **Tabs, open and the tab strip.** The registry and the actors own the
  tabs; the open routes (`service_open_*`, the prefetches) are
  `route_helpers.rs` helpers with one caller. Owner: P92.
  `ServiceState` is the desktop's own actor host. Owner: P84.
- **Settings.** Through the settings actor: on the structure.
- **Terminal, export, OS launch, window placement, app update, uninstall.**
  Front-end effects (decision 6): the terminal's PTYs, the export's file
  save, the launches and the release check stay in er-desktop. Their engine
  reads go through er-service in P83.2; `direct/os.rs` also asks `gh` for a
  PR's head branch outside `Ports` (`gh_pr_head_branch_name`), which P83.2
  moves onto the port. The release check's helpers go in P92.
- **The backend tick and the revision counter.** `resources/tick.rs` and
  `revision_watcher.rs` wake on `DesktopState.desktop_revision`, which
  sidebar refreshes, arena views, the browser annotation writes and sidebar
  commands bump. After P79 and P90 the arena view relay and the sidebar
  command are left. Owner: P84.
- **App-era prose.** About 300 comment lines in 90 files across er-desktop,
  er-service, er-engine and er-tui still describe the flag path, the App and
  its lock, `poll` and the snapshot build as live code, and desktop-ui's page
  types cite a `mergeChromeSnapshot` that no longer exists. Owner: P93.
- **App-era names.** `AppSnapshot` and the six Rust `*Snapshot` types:
  P86. desktop-ui's other `*Snapshot` interfaces: the open question.
- **Already done.** `TabState::adopt_key` has no hit in `crates/`. The two
  docs the exit list named are fixed; one code comment citing `commands.rs`
  is left (`pr_open_cache.rs`), for P93.

## Feature check

Decision 2 says phase 6 adds no new features. Each entry below changes
structure only, with these points where a user could notice something:

- P80.1 merges two reply writers. If the service's reply rows differ from
  MCP's for an anchored parent, the builder stops and the coordinator rules
  (the entry's main risk); with no ruling, nothing changes.
- P85 republishes the npm packages at 1.0.0 with the v1 storage path in the
  skills' text (decision 9), and the release notes state MCP's stability
  policy. Packaging and docs, no new behaviour.
- P88 moves the card asks onto the task host. Each done-when keeps the
  `jobs` row and the slot rule as today, so a user sees the same ask.
- P91 sends fewer `pr_status` pushes; the page shows the same values.

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
  D is the rest of `crates/er-desktop/src/**`; P87 to P90 each take only
  their own module and its helpers in `route_helpers.rs`.
- **T** `crates/er-tui/**`. Until #762 and #765 merge, their builder holds
  `service/shell.rs`, `service/loop_tests.rs`, `service/watched_tests.rs`,
  `service/config_hub_tests.rs`, the settle helpers in `service/tests.rs`
  and `docs/release-notes/tui-on-the-service.md`.
- **P** `crates/er-protocol/**` and the generated TypeScript (regenerated
  with `just protocol-gen`, never hand-merged).
- **U** `desktop-ui/src/**`. Only P86 and P93 (comments) take it.
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
  states decision 15's policy.
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
  loops; `DesktopState` holds the owner and no cache maps, and the fetch
  bookkeeping moves into the owner with them (`gh_user`, `gh_status_in_flight`,
  `gh_status_fetched`, `status_fetch_clock`, `pr_list_running`,
  `pr_list_clock`). The clock keeps its one-for-every-PR shape here; P91
  splits it.
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
  resolution and the feedback reader with its finding rows, and nothing MCP
  does not call; the free `gh`
  calls MCP makes go through `Ports.github`, and P78's deferred goldens are
  recorded; question and note replies and resolves have one writer in
  er-service, which MCP and the `ReplyToThread` and `ResolveThread`
  effects both call; every P78 golden is unchanged; a new ADR records the `headless` module:
  MCP calls er-service's functions and starts no actor (decision 8).
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

### P82 One `now_epoch_ms`

- Branch: `epic/p82-one-clock`. Lanes: C, E, S, D, M, and er-store and
  er-github. Model: `sonnet`. Depends on: P80.2, P92 (the desktop and MCP
  helpers have moved or settled), #764 (`tasks.rs`).
- **Done when:** `er_core::clock::{now_epoch_ms, now_epoch_secs}` exist
  (decision 12); every millisecond and seconds helper in the claims is
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
  on: #762 and #765 (`service/shell.rs`), P82.
- **Done when:** er-service re-exports the engine data types and pure
  helpers the front ends name and wraps the front-end-local effects
  (highlight, watcher, launch, copy, export, dir browse, update, uninstall,
  first launch, the view file) (decision 6); er-service gains `watch` and
  `highlight` features, off by default, that turn on er-engine's, so
  `build-service-headless` still checks the bare graph and `check-desktop`
  still checks each front end in its own graph; the TUI's watched-file reads
  go through an er-service function; er-tui's `[dependencies]` lose
  er-engine, er-git and er-store, and er-agent moves to dev-dependencies once
  a build confirms no production use (decision 13).
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
  Depends on: P83.1, P84 and P92 (all hold D; P84 deletes `ServiceState` and
  P92 moves the files whose imports this entry rewrites).
- **Done when:** er-desktop's `[dependencies]` and its macOS target table
  lose er-engine and er-store (dev-dependencies keep them); the reads the
  audit left in er-desktop reach the engine through er-service: the git
  reads of `direct/line_commits.rs`, `watcher.rs` and `tabs/follow.rs`,
  the preview reads of `resources/preview.rs`, the export render, the
  uninstall, and `direct/os.rs`'s `gh_pr_head_branch_name`, which calls
  `gh` outside `Ports` today and goes through the GitHub port.
- **Tests that change:** tests that name an engine type import it through
  er-service; no assertion changes.
- **Planted breaks:** a `use er_store::` left in er-desktop's `src` fails
  the build; a `gh` call outside the port in `direct/os.rs` fails a
  scripted-port test; `check-desktop` still passes.
- **Size:** about 450. **Fallback split:** none at the estimate.

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
  `ServiceState` is deleted, closing ADDENDUM-07 decision 3 (decision 10).
- **Done when:** `ServiceHost` takes a persisting `AppRegistry` and the
  app-records owner from its builder, while the TUI and the headless run keep
  the in-memory registry and no records; the desktop's revision relay and
  resource wake hang off `with_events`; the `load_requests` dedupe moves
  with it; `DesktopState.tasks` goes, since the host holds the task host;
  the bumps of `desktop_revision` that P79 and P90 leave (the arena
  view relay, the sidebar command) come from the host's events; every `ServiceState` test passes against the host.
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
  `sonnet`. Depends on: every entry above.
- **Done when:** ADR 0067's crate-graph section states the front-end rule,
  or a new ADR does and ADR 0067 cites it; ADR 0002 gains a line on the
  threaded `ServiceHost` the TUI runs (ADR 0073); docs-check passes both ways
  on every superseded mark; `crates/er-desktop/agent.md` describes the
  desktop on the service (no sidebar owner, no `route_helpers.rs`); the
  release notes cover MCP's stability policy and the skills' storage path;
  the `easy-review-mcp` and `@easy-review/skills` packages are at 1.0.0 and
  the skills' sources and built copies name the v1 storage path (decision
  9); `CONTEXT.md` changes only if a term changed, and none is expected.
- **Tests that change:** none.
- **Planted breaks:** a superseded mark without its line in the superseding
  ADR fails docs-check; a skill that still names the old storage path fails a
  new test beside `npm/skills/scripts/sync-skills.test.js`.
- **Size:** about 350. **Fallback split:** none, well under the line.

### P86 The `*Snapshot` renames

- Branch: `epic/p86-snapshot-renames`. Lanes: U, D, P, S. Model: `sonnet`.
  Depends on: P83.2 (lane D).
- **Goal:** the page type and the desktop's Rust types lose the
  `*Snapshot` name the deleted snapshot path gave them (decision 11).
- **Done when:** desktop-ui's `AppSnapshot` page type and every Rust
  `*Snapshot` type (`WatchStatusSnapshot`, `ReviewFlowSnapshot`,
  `FailedReviewFlowFinderSnapshot`, and the moved `GithubStatusSnapshot`,
  `RepoMergeSnapshot`, `WorktreeSnapshot`), with their desktop-ui mirrors,
  carry names from `CONTEXT.md`'s vocabulary, chosen on the issue; the
  er-protocol doc comment is updated and the TypeScript regenerated with
  `just protocol-gen`; the `command_shape` test inputs stay; no wire key or
  serialized field changes. desktop-ui's other `*Snapshot` interfaces are
  outside this entry (the open question).
- **Tests that change:** tests that name a renamed type follow the rename;
  no assertion changes.
- **Planted breaks:** a type renamed in Rust with its desktop-ui mirror left
  behind fails `check-ui`; a changed serialized field fails the existing
  wire tests and `protocol-check`.
- **Size:** about 500 (306 lines for `AppSnapshot`, 74 for the three that
  stay, 113 for the three that moved). **Fallback split:** P86a desktop-ui's
  `AppSnapshot`; P86b the Rust types and their mirrors.

### P87 The PR card's branch actions through the service

- Branch: `epic/p87-pr-actions`. Lanes: S, D. Model: `opus`. Depends on:
  P79.2 (the status refresh goes through the owner).
- **Goal:** merge, update branch, close and reopen, ready and draft, and
  delete and restore branch are er-service commands, as every other GitHub
  write is.
- **Done when:** `RunGithubPrAction` goes to an er-service command whose
  effect runs `er_engine::gh_pr_actions` through the port, off the actor;
  the status refresh after an action goes through the sidebar owner, so the
  card shows the result before the command answers, as today;
  `crates/er-desktop/src/gh_pr_actions.rs` keeps no `gh` call and is
  deleted if nothing else is left in it; refusal and error texts are
  unchanged.
- **Tests that change:** the desktop's PR-action tests send the command
  through er-service; assertions kept.
- **Planted breaks:** an er-desktop module that names `gh_pr_actions` fails
  a source scan; an action whose refresh skips the owner leaves
  `pr_status` unmoved and fails the refresh test; an action run on the
  actor thread fails the existing no-wait scan; a changed refusal text fails
  the existing message tests.
- **Size:** about 500. **Fallback split:** none at the estimate.

### P88 The card asks on the task host

- Branch: `epic/p88-card-asks`. Lanes: S, D. Model: `opus`. Depends on:
  #764 (`tasks.rs`, `runs.rs`).
- **Goal:** Ask AI, Validate, Elaborate and the AI-assisted finding reply
  run in er-service; the desktop sends a command and reads `jobs`.
- **Done when:** `ai_ask.rs` and `ask_context.rs` and the card helpers
  in `route_helpers.rs` (`card_action_prompt`, `card_model`,
  `card_progress`) move into er-service, which plans and runs the ask on
  its task host and lands the answer as `SubmitAiReply` or
  `AppendFindingResponse` with the context it captured; an ask takes no
  agent slot, as today (ADR 0021); the pending-reply set leaves
  `DesktopState` (`pending_ai_replies` and the `PendingAiReply` types in
  `page_view.rs`) and the `jobs` row an ask shows is unchanged; ADR 0074
  gains a consequence (card asks run on the host).
- **Tests that change:** `route_helpers/tests/ai_ask_tests.rs` and
  `ask_context_tests.rs` move with the code; assertions kept.
- **Planted breaks:** an er-desktop module that names `card_job` fails a
  source scan; an answer that lands on the active tab after a tab switch
  fails the captured-bucket test; a pending reply missing from `jobs` while
  the ask runs fails the jobs test; with the slot cap at one and a review
  running, an ask that waits for the slot fails the slot test.
- **Size:** about 700, counted in full (cut out of `route_helpers.rs` and
  moved across crates). **Fallback split:** P88a the asks on a thread and a
  finding; P88b the AI-assisted reply.

### P89 The desktop-only runs planned in er-service

- Branch: `epic/p89-desktop-runs`. Lanes: S, D. Model: `opus`. Depends on:
  P88 (both edit the run helpers in `route_helpers.rs` and er-service's
  runs).
- **Goal:** the diagram, the scoped review and a note sent to the agent are
  planned in er-service, as every other run is; the desktop writes no
  bucket file to start one.
- **Done when:** `runs.rs`'s `diagram`, `scoped` and `send_note` and
  their helpers (`note_prompt`, `send_note_blocked`, `scoped_diff`,
  `scoped_message`, `scope_stamps`, `spawn_scoped_reviewers`) plan in
  er-service; the `std::fs::create_dir_all` calls and
  `ensure_diff_artifacts` in `runs.rs` run in the service's run path, as
  P74 made them for the TUI; `runs.rs` keeps only the commands that name a
  tab and send; ADR 0074 gains a consequence that supersedes its line
  keeping these three runs in er-desktop.
- **Tests that change:** the run tests under `route_helpers/tests/` move
  with their code; assertions kept.
- **Planted breaks:** an er-desktop module that names
  `ensure_diff_artifacts` or calls `std::fs` in a run fails a source scan;
  a diagram with an unknown kind still refuses, and a plan that skips the
  check fails the existing refusal test; a scoped run that loses its stamp
  fails the existing stamp test; a send-note on a branch not checked out
  still refuses, and its existing test fails when the check is dropped.
- **Size:** about 800, counted in full. **Fallback split:** P89a the
  diagram and the send-note; P89b the scoped review.

### P90 Browser annotations through the workspace actor

- Branch: `epic/p90-ui-annotations`. Lanes: S, D. Model: `opus`. Depends
  on: #764 (`actor.rs`).
- **Goal:** `ui-annotations.json` and its screenshots have the workspace
  actor as their one writer, as every other bucket file does (ADR 0067).
- **Done when:** the five browser-annotation writes (add, delete, clear for
  a page, clear, update anchors) and the screenshot save are workspace
  commands whose effects write the bucket; `write_ui_annotation`,
  `delete_ui_annotation_in`, `clear_ui_annotations_in`,
  `save_annotation_screenshot_in`, `apply_anchor_updates` and
  `decode_data_url_png` leave `route_helpers.rs`; the `ui_annotations`
  clock and the `ui_annotations_io` lock leave `DesktopState`, the
  actor's queue taking the lock's place and a published revision the
  clock's; the read (`load_ui_annotations`, `read_annotation_screenshot`)
  comes from the actor's view; the page's commands and their answers are
  unchanged; ADR 0067 gains a consequence (the browser annotations joined
  the actor).
- **Tests that change:** `route_helpers/tests/ui_annotation_write_tests.rs`
  and the screenshot tests move to er-service; assertions kept.
- **Planted breaks:** an er-desktop module that calls `save_ui_annotations`
  fails a source scan; two adds sent together where one is lost fail the
  concurrency test; a write that does not move the published revision
  leaves the resource unpushed and fails the version test; a screenshot whose
  bytes differ on read fails the round-trip test.
- **Size:** about 900. **Fallback split:** P90a the annotation writes; P90b
  the screenshots and the read.

### P91 One `pr_status` version per PR

- Branch: `epic/p91-status-clock`. Lanes: S, D. Model: `sonnet`. Depends
  on: P79.2 (the fetch bookkeeping is in the owner).
- **Goal:** a status fetch moves only its own PR's `pr_status` version.
- **Done when:** `status_fetch_clock` becomes a version per
  `(owner, repo, number)` in the owner, and `resources/pr.rs` adds that
  key's version to the stored one; a fetch's start and end still reach the
  page as `fetching`.
- **Tests that change:** none.
- **Planted breaks:** a fetch of one PR that moves another PR's version
  fails the precision test; a fetch end that leaves its own version unmoved
  keeps `fetching: true` on the page and fails the existing end-of-fetch
  test.
- **Size:** about 250. **Fallback split:** none, well under the line.

### P92 Split `route_helpers.rs` and `page_view.rs` by caller

- Branch: `epic/p92-split-helpers`. Lanes: D. Model: `sonnet`. Depends on:
  P79.3, P84 and P87 to P91 (each takes its part of the two files first).
- **Goal:** both files are gone; each helper sits with the one module that
  calls it.
- **Done when:** what remains of `route_helpers.rs` moves to its caller:
  the tab-open routes (`service_new_tab`, `service_open_*`,
  `prefetch_pr_open_impl`, `prefetch_remote_pr_open_impl`, `new_tab_root`)
  to the `tabs` module or `resources/tabs.rs`; the release check
  (`app_update_cached`, `fetch_latest_release`, `check_app_update_inner`)
  to `resources/commands.rs`; `review_projection`, `review_revisions`,
  `finding_linked_thread_ids` and `read_annotation_screenshot` to the
  annotation resources; `after_github_write` and `review_comment_body` to
  `resources/github_writes.rs`; `feature_allows_mode_str` and
  `pr_cache_head_oid_for_pr` to `resources/content_routes.rs`;
  `image_data_url` to `resources/preview.rs`; what remains of
  `page_view.rs` moves the same way (`WatchStatusSnapshot` and its state to
  `watcher.rs`, `ReviewFlowSnapshot` and `FailedReviewFlowFinderSnapshot`
  to the flow resource); the tests under `route_helpers/tests/` move beside
  the modules they test; a helper that two modules call stays in one shared
  module named for what it does.
- **Tests that change:** the tests move; no assertion changes.
- **Planted breaks:** a scan test fails when `route_helpers.rs` or
  `page_view.rs` exists (planted by restoring one); the moved tests and the
  build prove the moves.
- **Size:** about 1,300, counted in full because functions are cut out of
  larger files. **Fallback split:** P92a the tab-open routes and their tests;
  P92b the rest and `page_view.rs`.

### P93 App-era leftovers

- Branch: `epic/p93-app-era-leftovers`. Lanes: D, S, E, T, U (comments
  only, plus the one dead field). Model: `sonnet`. Depends on: P86, P83.3,
  #762 and #765 (er-tui's `service/shell.rs` carries such comments).
- **Goal:** no code or comment describes the App, the flag or the snapshot
  path that phases 5 and 5b deleted.
- **Done when:** `DesktopState.branch_base_remote_oid` goes, with its write
  in `align_branch_base_probe` (the function goes too if the write is all
  it does) and its construction in `main.rs`; the comment in
  `crates/er-engine/src/pr_open_cache.rs` that cites `evict_lru` "in
  commands.rs" is corrected; comments that name the flag path, the App, its
  lock or mutex, `poll`, `poll_impl` or `build_snapshot` as live code are
  rewritten or removed in er-desktop, er-service, er-engine and er-tui
  (about 300 comment lines in 90 files), and so are desktop-ui's comments
  on the chrome-only snapshot and `mergeChromeSnapshot`, which no longer
  exists; `line_commits_cache.rs`'s header names the module that fills the
  cache; `comments-check` gains these phrases, so they cannot come back.
  `TabState::adopt_key` is already gone (no hit in `crates/`), so nothing
  is left to do for it. The plans and release notes under `docs/` that
  cite `commands.rs` are records of their time and stay.
- **Tests that change, up front:**
  `route_helpers/tests/content_route_tests.rs` drops its read of
  `branch_base_remote_oid`, which goes with the field; the probe test keeps
  its other assertions.
- **Planted breaks:** a comment that says "the flag path" in er-desktop
  fails `comments-check`; so does one that names `poll_impl`.
- **Size:** about 400. **Fallback split:** P93a Rust; P93b desktop-ui.

## The chain

Two tracks at a time, as in phase 5, then a closing sequence.

- **Starts on approval:** P78 and P79.1, beside each other (M against S,
  DS, D).
- **Track 1, MCP then the desktop audit:** P78 → P80.1 → P80.2 (also after
  P79.1) → P87 (after P79.2) → P88 (after #764) → P89 → P90 → P91.
- **Track 2, app records:** P79.1 → P79.2 → P79.3 → P84 (also after #764).
- **Closing sequence:** P92 (after P79.3, P84 and P87 to P91) → P82 → P83.1
  (after #762, #765) → P83.2 → { P83.3, P86 } → P93 → P85.
- **Lane S is shared.** P79.x, P80.1, P84 and P87 to P91 all edit er-service,
  mostly in new modules (app records, `headless`, card asks) with additions
  to `lib.rs`, `protocol.rs` and the actor's match. Two entries that edit it
  at once merge each other in; P84 runs alone while it edits `host.rs`.
- **Lane D is shared between the tracks.** P79.x hold DS and the sidebar
  functions of `route_helpers.rs`; P87 to P91 hold their own modules and
  their own helpers in that file. The cuts are in different functions of one
  file, so the second merge resolves deletions only.
- **The Depends-on lines** become each issue's Blocked-by line and carry
  every code dependency.

Totals: 15 entries, 20 PRs (more if a fallback split is taken), about 13,800
counted lines. P78, P79.2, P79.3 and P92 sit nearest the stop line and name
their splits.

## What phase 6 asks of the follow-ups and the release

- **#764 lands before P82, P84 and P88 to P90.** Its branch edits
  `flows.rs`, `host.rs`, `actor.rs`, `runs.rs` and `tasks.rs`.
- **#762 and #765 land before P83.1 and P93**, which rewrite er-tui's
  imports and comments in `service/shell.rs` and its tests.
- **The release** publishes `easy-review-mcp` and `@easy-review/skills` at
  1.0.0 after P85 (decision 9).
- **The post-1.0.0 CLI epic** starts from decision 2's list and the
  `headless` module P80.1 adds.
- **A main merge before P83.3** keeps er-mcp's manifest and the lint gate in
  step with any v0.6.x fix that reaches them.

## Risks

1. **MCP behaviour that skills see** (PLAN's risk). P78's goldens hold it;
   P80.1's writer merge is the one planned change of behaviour, and it stops
   for a ruling if the rows differ.
2. **Three writers of `projects.json` across processes.** P79.1 gives each
   process one writer; across processes the store's scope lock still orders
   them, as today.
3. **A large sidebar move and a large split.** P79.2, P79.3 and P92 cut
   functions out of `route_helpers.rs` and `page_view.rs` and count in full;
   each names its split.
4. **Superseded placements.** P89 and P90 move what ADR 0074 and P58.2 kept
   in er-desktop because only the desktop has it. Each adds a consequence to
   the ADR, so the earlier reason is not read as current.
5. **A visible change slipping into a structural move** (decision 2). The
   card asks' `jobs` row and slot rule (P88), the PR actions' messages (P87)
   and the browser annotations' answers (P90) each have a test that holds
   today's behaviour; the feature check above lists what a user could notice.
6. **er-service grows a front-end surface.** Decision 6's re-exports widen
   er-service's public API. P83.3's rule keeps new engine calls from front
   ends passing through it; the reviewer reads each new re-export.
7. **Feature unification.** er-service's `watch` and `highlight` features
   could leak into the headless build through a workspace build.
   `build-service-headless` and `check-desktop` keep each graph checked on
   its own.
8. **Machine load.** Two tracks run at once, plus a C-only entry; heavy
   checks take the lock.

## ADR and doc changes

- P79.1: a consequence on ADR 0070 (the inbox and projects followed its
  home).
- P80.1: a new ADR for the `headless` module (MCP calls er-service and
  starts no actor); it records ADDENDUM-07 decision 9 as done.
- P88 and P89: consequences on ADR 0074 (card asks on the host; the three
  desktop-only runs planned in er-service).
- P90: a consequence on ADR 0067 (the browser annotations joined the actor).
- P85: ADR 0067's crate-graph section (or a new ADR it cites) states the
  front-end rule; ADR 0002 gains the threaded-host line;
  `crates/er-desktop/agent.md`, the release notes and the npm packages.
- No `CONTEXT.md` term changes. The owner and module names are code names.
