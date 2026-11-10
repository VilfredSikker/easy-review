# Addendum 04: phase 3, jobs and workflows

Written at the phase-3 gate (#396), following [PLAN.md "Re-planning at a phase gate"](./PLAN.md#re-planning-at-a-phase-gate). Where this file disagrees with [PLAN.md](./PLAN.md) or an issue body, this file wins; [ADDENDUM-01](./ADDENDUM-01-decisions.md) and [ADDENDUM-02](./ADDENDUM-02-issue-decisions.md) (D16–D18) still bind.

Drafted by the D1 session, design-reviewed in three rounds (amendments A1–A7, last edits L1–L5), and owner decisions D16–D18 are folded in. The claims were checked against `release/v1.0.0` at `c02634c80` and re-checked at the gate against `release/v1.0.0` at `76d89a66f`, after P20, P21 (PR #463), P24, #441 and #467 (`21591290c`) merged. The last section records what the re-check found; the full change list is on #396.

**Stop line for phase 3 (L5, adopted by the coordinator):** pure deletions are measured by what remains and do not count toward the 1,500-line stop line. PLAN's rule otherwise stands.

## Claims (corrected)

- **Spawn paths — six.** Card AI `app/card_ai_spawn.rs:223` (slot `:194`, resolver `:46`); background worker `app/state/comments.rs:4145`, spawn `:4221`; tab-local worker `:3287`, spawn `:3377`; shell `spawn_command` `:2879`, `sh -c` `:2943`; arena `arena/adapter.rs:145`; model discovery `model_discovery.rs:133`, which runs the catalog's agent CLIs (`ai_hub_catalog.toml:57,135,168`). Three argv resolvers: `agent_runtime::resolve_invocation` (card only; `agent_runtime.rs:185`), inline code in `comments.rs`, `arena/adapter.rs:33`. The shell path runs `[commands].summary` through `resolve_command` (`er-tui/src/input/mod.rs:452`, `config/mod.rs:1561`).
- **Decoders — three live final-text decoders** plus one log decoder and one raw parse: card `card_ai_spawn.rs:421`; arena `arena/adapter.rs:230`, `:261`; diagrams `ai/diagrams.rs:341` via `comments.rs:4362`; log parser `state/mod.rs:9601`; importance raw stdout `comments.rs:4344`.
- **Kill paths — two.** `AgentRunHandle` creates a process group (`agent_run.rs:247`–`:256`), signals it (`:338`–`:380`), kills in `Drop` (`:398`–`:412`); the arena registry calls `child.kill()` on each pid (`arena/registry.rs:19`–`:27`).
- **Slot cap — seven production acquisition sites.** `card_ai_spawn.rs:194`; `comments.rs:2934`, `:3294`, `:4155`; `arena/orchestrator.rs:1114`, `:1311`, `:1728`. A test also holds slots to stand in for an arena run (`comments.rs:4986`). ADR 0021's consequence stands: the cap holds because each site acquires.
- **Agents write sidecars directly.** Prompts tell the agent to `Write`: general review `prompts.rs:256`–`:258`; expert `:409`, `:588`, `:624`; tour `:683`; professor `:983`; triage `:1248`; questions `:1422`, `:1774`; validation `:1466` (P21 replaced the two review rewrites; the validate prompt writes `validation.json` and says not to write `review.json`); github-comments `:1585`; summary `:1625`, `:1706`; notes `:1842`, `:1907`. Diagrams and importance are host-written (`prompts.rs:790`–`:792`, `:822`, sentinel `:1031`–`:1032`). The in-tree MCP upload commits through the store under one scope lock (`sidecar_upload.rs:235`–`:273`, `:458`–`:459`).
- **Bare kinds — every agent-written kind.** Review (`ai/review.rs:89`), expert (`ai/experts.rs:284`), professor (`ai/professor.rs:37`), triage (`ai/triage.rs:52`), tour (`ai/review.rs:597`), questions, notes and github-comments (`ai/comments.rs:406`, `:418`, `:430`), arbiter (`ai/arbiter.rs:68`), order and checklist (`checklist_store.rs:19`, `:31`), validation (`ai/validation.rs:80`), and P21's scoped review, professor and expert (`ai/scoped_overlay.rs:115`, `:127`, `:139`). `diagram` is Enveloped (`ai/diagrams.rs:60`); `summary` is a text kind (`store_registry.rs:51`).
- **Arena.** Adapter/argv/retry `adapter.rs:33`, `:81`–`:99`, `:145`; registry and pid-only kill `registry.rs:19`–`:27`, `:39`–`:43`; `progress.jsonl` `storage.rs:33`, `:195`, `:261`; import rewrite `import.rs:95`. `arena/` is 6,593 lines; with `app/state/arena.rs` (1,339) it is 7,932.
- **Workflow state.** The review flow persists no workflow record; only the arena persists its run. P21 added a `Verifying` stage to the flow state machine (`ai/review_flow.rs:17`–`:21`, `:57`–`:63`, `:122`–`:128`), and it is not durable either.
- **Importance.** Sentinel markers `prompts.rs:1031`–`:1032`, prompt block `:1068`/`:1087`; parsed `config/risk_rules.rs:348`; merged into global config `:359`–`:368`; called `comments.rs:4343`–`:4357`; config path `config/mod.rs:1676`–`:1687`.
- **D1.** `ai/browser_annotations*` and `ai/browser_prompt.rs` have no caller (`ai/mod.rs:2`–`:3`, `:34` only; header `browser_annotations.rs:3`); 1,803 lines including `browser_prompt/tests.rs`.
- **Registry.** Engine registry `store_registry.rs:15`–`:37`; session kinds `ui`-gated `:27`; `validation` and the three scoped kinds register at `:47`–`:50`; `summary` at `:51`. The five desktop records register in their own crates (`tabs.rs:68`, `inbox.rs:168`, `pr_cache.rs:39`, `gh_status_cache.rs:65`, `pr_open_cache.rs:42`) and nothing composes them (the only caller is a test, `projects.rs:1370`–`:1374`); `unaccounted` is test-only (`store.rs:1158`, `:1173`, `:1199`, `:1267`, `:1285`; `lock.rs:791`).
- **Raw readers/writers outside the store:** `review_flow/results.rs:41` (the atomic helper) and `:166`, with the rollback at `:47`–`:61`; `review_flow_output.rs:124`, `:216` (archive renames), `:247`–`:270`, `:330`–`:338` (`read_sidecar`); `comments.rs:2488` and `:2509`–`:2510` (the clipboard reads), `:2807` (the completion summary reads `review.json`); `arena/seeded.rs:130`; `arena/import.rs:102`–`:105`.
- **Agent storage grant:** `inject_agent_storage_access` (`config/mod.rs:1291`) gives every agent `--add-dir <bucket>`.

## `er::agent_spawn` matcher (A1)

There is no `#[expect(er::agent_spawn)]`; an unregistered tool is E0710. The rule has two file lists under `[workspace.metadata.lint-gate]`:

- `agent-spawn-os-launch`: permanent, reviewed, not shrink-checked. Exactly one OS-launch module per crate: `crates/er-engine/src/os_launch.rs` takes the clipboard (`comments.rs:2696`, spawn `:2715`), the editor (`app/state/navigation.rs:849`) and uninstall's `sh`/`ps`/`powershell` (`uninstall.rs:461`, `:490`, `:499`); `crates/er-desktop/src/os_launch.rs` takes the editor and openers (`commands.rs:1655`, `:2453`, with the remaining opener arms at `:2466`–`:2475`); `crates/er-tui/src/os_launch.rs` takes `tar` (`update.rs:104`).
- `agent-spawn-allow`: shrink-only, the agent call sites. A file leaves when its last reported spawn is gone: `card_ai_spawn.rs` and `model_discovery.rs` at P27, `comments.rs` at P29, `arena/adapter.rs` at P33a. **P33b deletes the list** (the one PR that does), and its stale check then passes.

The matcher reports every `Command::new(<program>)` outside `crates/er-agent` and the OS-launch files, whether the program is a literal or an expression. Literal `git` and `gh` are left to `er::git_spawn` and `er::gh_spawn`. `cfg(test)` and `#[test]` code are skipped.

Two more rules (L1):
- The tool crates `crates/er-lint-gate` and `crates/er-crap` are exempt, since they spawn `cargo` themselves (`er-lint-gate/src/main.rs:299`).
- The gate itself checks the `agent-spawn-os-launch` list: every entry must be `crates/<crate>/src/os_launch.rs`, with at most one per crate, so the carve-out cannot grow by review oversight.

## PR entries

Shared acceptance: `fmt-check`, `clippy`, plus `test`/`test-desktop` as the touched crates require; entries touching `er-store`, `er-agent` or features add `test-all`, and `build-engine-headless` or `build-mcp` where the headless build is affected. The stop line is 1,500 added-plus-changed lines after rename detection; pure deletions are measured by what remains and do not count toward it.

### P26a Create er-agent (runtime and the guardrail)

- Branch: `epic/p26a-er-agent-runtime`. Lanes: C, E, F (it moves the desktop openers out of `commands.rs`). Model: `opus`. Depends on: #441 (it touches the tests this PR moves; L4) and #468 (the script-writing test helper, so moved tests do not carry the flake); phase 2 is closed at the gate.
- **Goal:** one process runtime, one slot pool, the OS-launch carve-outs and the gate rule that makes a later agent spawn outside `er-agent` fail.
- **Done when:**
  - `crates/er-agent` holds `agent_run`, `agent_slots` and `agent_timing`; `er-engine` re-exports them until P33b removes the shims (L3); `er-agent` has no dependency on `er-engine`.
  - The three OS-launch modules exist and hold the clipboard, editor, uninstall and `tar` spawns; `er::agent_spawn` fails a planted non-literal `Command::new(&cmd)` in `sync.rs` and passes the same code in `os_launch.rs`; `comments.rs` keeps its agent-list entry because its tab and shell spawns remain.
  - The tool crates `er-lint-gate` and `er-crap` are exempt, and the gate rejects an `agent-spawn-os-launch` entry that is not `crates/<crate>/src/os_launch.rs` or is a second one for a crate (L1).
  - The process-group kill leaves no grandchild; a pid-only kill test leaves one alive, which is the proof P33b keeps the group path.
- **Planted breaks:**
  - a non-literal `Command::new` in `sync.rs` fails the gate;
  - a pid-only kill test fails on the surviving grandchild;
  - a clipboard or editor spawn outside `os_launch.rs` fails the gate;
  - a second OS-launch entry for one crate, or one at another path, fails the gate.
- **Fallback split:** P26a.1 the crate and moved runtime; P26a.2 the OS-launch modules and the rule.
- Size: M.

### P26b ProviderSpec, one decoder, JobSpec/JobRunner

- Branch: `epic/p26b-agent-jobs`. Lanes: C, E. Model: `opus`. Depends on: P26a.
- **Goal:** a config-free job model the engine can build.
- **Done when:**
  - `ProviderSpec` (program, args, env, output protocol, timeout) is built by the engine; `er-agent` stays free of `crate::config`, `crate::ai` and `crate::app` (the cycle check fails the build otherwise).
  - One final-text decoder serves the card, arena and diagram shapes; a table test feeds each live shape (`card_ai_spawn.rs:421`, `arena/adapter.rs:230`, `ai/diagrams.rs:341`).
  - `JobSpec`/`JobRunner` run a scripted provider end to end, and the runner acquires the slot.
- **Planted breaks:** adding `crate::config` to `er-agent` breaks the build; a decoder that drops the diagram sentinel fails the table case; a job that spawns without acquiring waits on a full pool (the acquisition test fails if removed).
- **Fallback split:** P26b.1 `ProviderSpec`/decoder; P26b.2 `JobSpec`/`JobRunner`.
- Size: M.

### P27 Card AI and model discovery onto the Provider

- Branch: `epic/p27-card-ai-job`. Lanes: E, F. Model: `opus`. Depends on: P26b.
- **Goal:** card AI is a job and model discovery runs through the `Provider` with no slot.
- **Done when:** `run_card_ai_subprocess` is a `JobRunner` call with `Stdout<CardReply>`; the desktop command returns the same reply and drains progress off the blocking path; `card_ai_spawn.rs` is gone; `run_models_command` resolves through the `Provider` and takes no slot; the allow-list loses both files.
- **Planted breaks:** a direct `Command::new` in `commands.rs` fails the gate; an undrained progress channel deadlocks a blocking call (the test fails before the drain fix).
- Size: M.

### P28a Background queue onto the JobRunner

- Branch: `epic/p28a-background-jobs`. Lanes: E, W. Model: `opus`. Depends on: P27.
- **Goal:** the background queue submits jobs; the runner owns slots.
- **Done when:** `enqueue_background_agent_task` (`comments.rs:3821`) and its worker (`:4145`) submit `JobSpec`s; the queued/cancel pills and `BackgroundTask` identity are unchanged; triage, review, expert, professor, tour, summary, validate and diagram kinds are ported; the runner is the only slot acquirer for these; the allow-list loses the background entry.
- **Planted breaks:** removing the runner's acquisition lets an N+1th job run (the pool test fails); a failed host write for importance/diagram surfaces as a failed job.
- **Fallback split:** P28a.1 queue and worker; P28a.2 the kinds and host writes.
- Size: L.

### P28b Review-flow launch and the Deep-run cancel/orphan test

- Branch: `epic/p28b-flow-launch`. Lanes: W. Model: `opus`. Depends on: P28a.
- **Goal:** the review flow launches jobs, and a Deep cancel cannot leave orphans.
- **Done when:** `review_flow.rs:321` submits jobs; one test cancels a Deep run mid-run across the runner and the arena registry and asserts every child is dead; the runner and arena progress channels are named and both stop on cancel.
- **Planted break:** removing the arena cancel mapping leaves a child alive; the test fails.
- Size: M.

### P29 Tab-local prompts, the shell path and the summary job (D18)

- Branch: `epic/p29-tab-jobs`. Lanes: E. Model: `opus`. Depends on: P28b.
- **Goal:** the last in-process spawn paths are jobs, summary is a built-in job, and `comments.rs` leaves the allow-list.
- **Done when:**
  - `spawn_agent_prompt` (`comments.rs:3152`) and `spawn_command` (`:2879`) submit `JobSpec`s; the tab's `command_status` and logs read runner events; `comments.rs` leaves the allow-list.
  - D18: delete `commands.summary` (`config/mod.rs:71`) and its `resolve_command` arm (`config/mod.rs:1561`); the TUI's `RunCommand("summary")` submits `JobKind::Summary`; the retired key joins the first-launch notice (config has no `deny_unknown_fields`, so the notice is the only signal).
  - A test shows the summary hub action submits the job kind and `sh -c` never sees it. The shell job kind remains for user package commands and keeps its slot (it can launch provider CLIs, as at `comments.rs:2934`).
- **Planted breaks:** a `Command::new` in `comments.rs` fails the gate once the file is off the list; a config that still sets `[commands].summary` gets the notice and the built-in job still runs; a summary routed through `sh -c` fails the test.
- Size: M.

### P30a Host commits job outputs

- Branch: `epic/p30a-job-commit`. Lanes: E, C. Model: `opus`. Depends on: P28a, P29, P24, #464 (the store-lock fix lands first).
- **Goal:** agents write to staging; the host validates and commits through the store.
- **Done when:**
  - Every job's output is parsed and validated with the upload path's `validate_contents_before_write` (`sidecar_upload.rs:180`–`:229`, called at `:449`), against type, version and `diff_hash`, and committed through `ArtifactStore`; a malformed output fails the job with a reason.
  - `safe_output_dir` and `--add-dir` point at staging, and the inputs an agent reads are staged (`inject_agent_storage_access`, `config/mod.rs:1291`).
  - Update kinds (`questions`, `notes`, `github-comments`) use a patch contract: the host reads `current` under the store lock, applies the agent's changes by id, and keeps concurrent edits.
- **Planted breaks:** tampered staged output fails validation; a concurrent edit written between staging and commit is lost without the patch contract; a direct bucket write is not committed.
- **Fallback split:** P30a.1 staging, validation and commit; P30a.2 the update-kind patch contract.
- Size: L.

### P30b.1 Port the remaining raw readers/writers onto the store

Split from P30b at the gate (coordinator): joining the deny list is file-granular, which grew the scope.

- Branch: `epic/p30b1-raw-readers`. Lanes: E, W, C. Model: `opus`. Depends on: P30a, P33b, P20, P21, P24.
- **Goal:** no raw read or write of an agent kind outside the store.
- **Done when:**
  - `review_flow_output.rs:124`, `:216`, `:247`, `:330`, `review_flow/results.rs:41`, `:166`, `comments.rs:2488`, `:2509`–`:2510`, `:2807`, `arena/seeded.rs:130`, `arena/import.rs:102`–`:105` read and write through the store; those files join `sidecar-fs-deny`.
  - The list is file-granular and flags every `fs::`, `File::` and `OpenOptions::` path in a file, so more than the sidecar payloads converts. `review_flow_output.rs`'s archive and restore (`:124`, `:135`, `:216`, plus `create_dir_all`, `metadata`, `set_permissions` and `remove_file` around them) need a store-level archive. `comments.rs` also carries the clipboard reads, the legacy-root marker (`:2791`, `:2793`), four `create_dir_all` calls and two debug-log writes (`:3474`, `:4316`), which need a helper outside the list or a store API.
  - `ai/overlay_files.rs` stays the one raw helper, for the non-registered `.prev.json` and `review-files.txt` files.
- **Planted breaks:** a raw `fs::read` in `review_flow_output.rs` fails the sidecar rule; an arena import parses the review through the store, and falling back to `empty_review` fails a test.
- Acceptance adds `test-all`.
- Size: L.

### P30b.2 Envelope every agent kind; delete `Mode::Bare`

- Branch: `epic/p30b2-bare-deletion`. Lanes: E, C. Model: `opus`. Depends on: P30b.1.
- **Goal:** bare payloads are gone from storage.
- **Done when:**
  - Every kind still Bare is ported to `Enveloped`: review, expert, professor, triage, tour, questions, notes, github-comments, arbiter, order, checklist, validation and the three scoped overlays (`ai/review.rs:89`, `:597`; `ai/experts.rs:284`; `ai/professor.rs:37`; `ai/triage.rs:52`; `ai/comments.rs:406`, `:418`, `:430`; `ai/arbiter.rs:68`; `checklist_store.rs:19`, `:31`; `ai/validation.rs:80`; `ai/scoped_overlay.rs:115`, `:127`, `:139`). `diagram` is already Enveloped (`ai/diagrams.rs:60`). The "stay Bare in 1.0" comments (`ai/comments.rs:398`–`:401`, `checklist_store.rs:4`) change with it.
  - `summary` is a host-committed text artifact (`store_registry.rs:51`).
  - The `Mode::Bare` variant is deleted from `er-store/src/artifact.rs`.
  - An MCP upload of a bare `tour.json` still commits: the upload path parses the payload and commits it through the store, which writes the envelope (the installed skills keep working).
- **Planted breaks:** a bare payload that is not an upload is rejected by the store; a bare tour upload fails unless the upload path still accepts bare payloads.
- Acceptance adds `test-all` and `build-mcp`.
- Size: M.

### P31 Importance as a RiskRules artifact

- Branch: `epic/p31-importance-artifact`. Lanes: C, E. Model: `opus`. Depends on: P30b.2.
- **Goal:** importance output is a store artifact with a repo scope.
- **Done when:** `er-store` gains `Scope::Repo` (`key.rs:125`), reviewed as a key-layout API change; the job commits a `RiskRules` artifact under the repo scope; filters and Settings read it merged with `config.toml [risk]`; `RiskRuleProposal::merge_into_global_config` is gone.
- **Planted breaks:** a proposal for another repo is refused; a job that writes the config file fails; a filter still reading the old table finds nothing.
- Size: M.

### P32a Workflow record and crash resume

- Branch: `epic/p32a-workflow-record`. Lanes: W. Model: `opus`. Depends on: P28b.
- **Goal:** a durable workflow record that replaces `App.review_flow_leases` (`app/state/mod.rs:5938`) and the persisted arena status, with the coordinator's resume policy.
- **Done when:**
  - Each stage stores status (`Pending|Running|Complete|Failed|Interrupted`), output artifact id and diff generation; Quick/Standard/Deep and the arena launcher are stage lists; the stages include P21's `Verifying` pass (`ai/review_flow.rs:17`–`:21`, `:122`–`:128`); the record uses P23's store lease (`er-store/src/lock.rs:147`, no engine caller today).
  - On load, an interrupted record is marked `Interrupted` and the user is offered a re-run of the unfinished stages; **nothing spawns on load**. Independent jobs re-run per job, cross-voting stays atomic, and a changed diff generation starts over. There is no auto-resume.
  - A stale `Running` stage with a dead lease becomes `Interrupted` rather than blocking forever.
- **Planted breaks:** a crash after `Finders` re-runs `Triage` (stage-skip test fails); a load that spawns fails the no-spawn test; a stale `Running` stage blocks forever without lease expiry; the same diff generation resumes while a changed one restarts.
- **Fallback split:** P32a.1 the record and lease; P32a.2 resume, notice and the load test.
- Size: L.

### P32b Panel stage and the free round measurement (D17)

- Branch: `epic/p32b-panel-stage`. Lanes: W. Model: `opus`. Depends on: P32a.
- **Goal:** the Panel stage runs reviewers as jobs and measures round two at no extra cost.
- **Done when:** each reviewer is a `JobSpec`; a `PanelRun` artifact records rounds, round-1 votes and the round-2 outcome; the measurement is the free proxy (does round 2 change `quorum()` against round-1 votes?), with no extra arbiter call and the default unchanged; cross-voting is atomic; a Deep cancel leaves no orphans.
- **Planted breaks:** a panel that rewrites `review.json` fails the read-only assertion; an extra arbiter call fails the call-count test; a `PanelRun` without the proxy field fails its schema test; a cancel mid-round leaves a child, and the orphan test fails.
- **Fallback split:** P32b.1 the Panel stage; P32b.2 the measurement field and its schema test.
- Size: L.

### P33a Arena reviewers and arbiter onto the JobRunner

- Branch: `epic/p33a-arena-jobs`. Lanes: W. Model: `opus`. Depends on: P32b.
- **Goal:** arena calls are jobs; its adapter, retry and decoder are gone.
- **Done when:** `arena/adapter.rs`'s spawn, argv, retry and decoder are deleted; every reviewer and arbiter call is a `JobSpec`; one progress channel remains; the allow-list loses `arena/adapter.rs`; quorum, seeded arbitration, identity and provenance are pinned by the existing tests before deletion.
- **Planted break:** an arena spawn outside `er-agent` fails the gate; a retry test shows the runner's retry.
- **Fallback split:** P33a.1 reviewers as jobs; P33a.2 the arbiter as a job.
- Size: L.

### P33b Delete the arena registry, pid kill, `progress.jsonl` and glue

- Branch: `epic/p33b-arena-deletion`. Lanes: W. Model: `opus`. Depends on: P29, P33a.
- **Goal:** one kill path and one progress channel; the slot API closed; the allow-list deleted.
- **Done when:** `arena/registry.rs:19` and its pid-only kill are gone; `progress.jsonl` (`arena/storage.rs:33`) is gone; `app/state/arena.rs` shrinks to the workflow adapter; `agent_slots::acquire` is private to `er-agent` and unreachable from the engine; the engine shims are gone; the `agent-spawn-allow` list is empty and **deleted here** (the one PR that deletes it).
- **Planted breaks:** a grandchild survives a cancel if a pid-only path remains; an engine call to `acquire` no longer compiles; a stale allow entry fails the gate.
- Size: M.

### P34a Accepted findings and the projection

- Branch: `epic/p34a-accepted-findings`. Lanes: W, E. Model: `opus`. Depends on: P33b, P20. Holds E; runs after the E chain.
- **Goal:** accepting a candidate writes its own artifact; the projection reads it.
- **Done when:** an `AcceptedFinding` artifact is written on acceptance; the findings projection (`ai/projection.rs:29`–`:58`) reads it through the `accepted_provenance` input alongside the general review; ids and order match the front ends, asserted by extending `crates/er-engine/tests/findings_projection.rs` and its eight goldens (`tests/golden/findings_projection/case_*.json`); the arena run no longer rewrites `review.json`.
- **Planted break:** a run that rewrites `review.json` fails the golden test; a projection that ignores the artifact hides an accepted finding.
- Size: M.

### P34b Delete the arena import, the rollback and the provenance run read

- Branch: `epic/p34b-import-deletion`. Lanes: W, E. Model: `opus`. Depends on: P34a, P21. Holds E; runs after P34a.
- **Goal:** the arena import path and the raw review rewrites are gone.
- **Done when:** `arena/import.rs:95`, the `review_flow/results.rs:47`–`:61` rollback and the `accepted_provenance` run read are deleted.
- **Planted break:** an arena import cannot overwrite the general review; the old path is absent and the golden review test proves the deletion did not change behaviour.
- Size: M.

### P35 D1: delete the browser annotation modules

- Branch: `epic/p35-browser-annotations`. Lanes: E. Model: `sonnet`. Depends on: none. Runs before P26a or between P27 and P28a.
- **Goal:** remove the modules D1's gate condition finds unused.
- **Done when:** `ai/browser_annotations*` and `ai/browser_prompt.rs` (1,803 lines, including `browser_prompt/tests.rs`) and their `ai/mod.rs:2`–`:3`, `:34` lines are gone; `docs-check` passes after the browser-rework docs stop naming them.
- **Planted break:** any remaining reference fails the build; the doc check fails if a guide still links the modules.
- Size: S.

### P36 Registry gate

- Branch: `epic/p36-store-registry`. Lanes: E, C, F. Model: `opus`. Depends on: P17, P22. Holds E; runs after P37 or in an E gap.
- **Goal:** one registry per front end, with the same kinds, and a real `unaccounted` check (M12).
- **Done when:** the session payloads leave the `ui` gate (`session_store` moves out of `app/state`, `app/state/mod.rs:15`; the TUI writes session state through `App`, `state/mod.rs:5521`, `:5764`); the desktop contributes its five records; MCP skips `unaccounted` on app scope; a per-front-end test asserts the composition; a debug/test startup runs `unaccounted`.
- **Planted breaks:** an unregistered kind fails the per-front-end test; a desktop-only composition fails the TUI session test.
- Size: M.

### P37 Split the agent config into `agents.toml` (D16, A7)

- Branch: `epic/p37-agents-toml`. Lanes: C, E, F. Model: `opus`. Depends on: P31. Holds E.
- **Goal:** the D16 split, safely: agent settings move to `agents.toml`; every other setting is carried over unchanged.
- **Done when:**
  - `v1/agents.toml` holds providers and model groups; the compiled-in catalog still merges (`config/mod.rs:734`–`:736`, `:766`) with its tombstones. `config.toml` keeps preferences, risk rules in `[risk]`, and every `[commands]` entry (test, lint, typecheck, security, custom commands); A4 drops custom commands from the move.
  - The move carries every agent and provider field over field-for-field; no setting is dropped or rewritten.
  - If `config.toml` does not parse, neither file is written, a notice is shown, and later saves are refused until it loads.
  - `agents.toml` is written atomically, read back and compared, and only then is `config.toml` rewritten, with a timestamped backup. A crash between the two writes leaves a state a restart completes with nothing lost.
  - When both files carry agent sections, `agents.toml` wins over the leftovers.
  - A second run changes neither file.
- **Tests, each proved with a planted break:**
  1. field-for-field migration — plant: drop one field, the migration test fails;
  2. an unparseable `config.toml` stays byte-identical and no `agents.toml` is created — plant: overwrite it, the test fails;
  3. a crash between the two writes, then a restart completes the move with nothing lost — plant: drop the staged `agents.toml`, the restart test fails;
  4. no overwrite on save after a failed load — plant: save defaults, the test fails;
  5. a second run changes nothing — plant: re-read agent keys from `config.toml`, the test fails;
  6. `.invalid` backups do not overwrite each other — plant: reuse one name, the test fails;
  7. an unparseable `agents.toml` stays byte-identical and saves are refused — plant: let a Settings save write default providers over it, the test fails (L2);
  8. a read-back mismatch leaves `config.toml` untouched — plant: truncate the `agents.toml` write, the test fails if `config.toml` was rewritten (L2);
  9. with agent sections in both files, `agents.toml`'s values load — plant: prefer the leftover `config.toml` section, the test fails (L2).

  Test 1's fixture sets every agent field to a non-default value, so dropping a field that held its default cannot pass (L2).
- Builds on the `main` fix for #459 (a `config.toml` that fails to parse is no longer overwritten on save), merged into `release/v1.0.0` by #467 (`21591290c`). On release the guard is `config/mod.rs:1691`–`:1760` (`CONFIG_LOAD_BANNER` and the failed-load set) and `save_config_to` at `:1939`–`:1955`, which refuses while the path is recorded and re-parses before writing. P37 extends the same guard to `agents.toml` and the two-file split.
- Size: M.

## Lanes and schedule

| Lane | Hot files |
|---|---|
| **E** | `app/state/mod.rs`, `app/state/comments.rs`, `card_ai_spawn.rs`, `agent_run.rs`, `agent_runtime.rs`, `agent_slots.rs`, `ai/prompts.rs`, `ai/diagrams.rs`, `ai/validation.rs`, `ai/scoped_overlay.rs`, `config/mod.rs`, `config/risk_rules.rs`, `app/state/session_store.rs` |
| **W** | `arena/**`, `app/state/arena.rs`, `app/state/review_flow.rs`, `app/state/review_flow/**`, `app/state/review_flow_output.rs` |
| **F** | `er-desktop/src/commands.rs`, `er-desktop/src/main.rs`, `er-tui/src/input/mod.rs`, `desktop-ui/src/lib/types.ts` |
| **C** | root `Cargo.toml`, `Cargo.lock`, `justfile`, `scripts/docs-check.sh`, `crates/er-lint-gate/**`, `crates/er-store/src/**`, new crate manifests |

Schedule (three lanes at once is the ceiling):

```
1. P35 → P26a → P26b → P27 → P28a → P28b → P29
2. E: P30a            W: P32a → P32b → P33a → P33b        (parallel; different lanes)
3. P30b.1 → P30b.2 (E, W, C) after P30a and P33b
4. P31 (C, E) → P37 (C, E, F)
5. P34a → P34b (W, E) after P37, P33b, P20, P21
6. P36 (E, C, F) after P37, with P17 and P22 closed
```

P30b.1 and P30b.2 hold E, W and C, so they wait for both the P30a edge and the W chain through P33b. P34a/P34b hold E and wait for P37. P36 holds E, C and F and waits for P37. P35 runs before P26a. P29 and P33b both need P28b/P29 as written; the allow-list is deleted only in P33b.

## Risks

Known:
1. **External agents and skills writing sidecars directly.** After P30a a direct bucket write bypasses staging, validation and the envelope; they must use MCP upload. P24 landed: the in-tree MCP upload already commits through the store under one scope lock (`sidecar_upload.rs:235`–`:273`, `:458`–`:459`).
2. **D5 deletion changes on-disk formats.** Agent-written kinds become enveloped; external readers must go through the store. This belongs in the release notes.
3. **Deep runs span two job systems until P33b.** The D17 proxy starts when P32b's Panel runs.
4. **Workflow state needs a new record.** The review flow has none today; P32a is where resume becomes possible.

From the re-check:
5. **The shell path is a sixth spawn path.** `spawn_command` (`comments.rs:2879`) runs `sh -c`, and `[commands].summary` reaches it (`input/mod.rs:452`, `config/mod.rs:1561`). D18 retires the summary key; the shell job kind remains for package commands and keeps its slot.
6. **Three live decoders.** Card, arena and diagrams; `agent_runtime::decode_final_text:421` is dead. The one decoder comes from the live shapes.
7. **Seven production slot sites.** The runner owning the slot is what makes ADR 0021's consequence “by construction”; P33b closes the API.
8. **Registry divergence is latent.** `unaccounted` is test-only today; P36 makes it a debug/test startup check.
9. **Prompt and validator drift.** When P30 changes a contract, the `schemars` schema, the prompt output section and the validator change in one PR.
10. **Machine load.** P26a, P28a, P30a, P30b.1, P32a, P32b and P33a are L; three lanes at once is the ceiling.

Added:
11. **`[commands].summary` (D18).** Retiring the key is a user-visible change; the notice is the only signal because config ignores unknown fields. The built-in job keeps summary working.
12. **Update kinds.** `questions`, `notes` and `github-comments` are user-edited and agent-rewritten; staging the whole file loses concurrent edits. P30a's patch contract and its concurrent-edit test are the answer.
13. **Deep-run cancel/orphan.** Between P28b and P33b a Deep run spans the runner and the arena registry, which lacks a group kill; two progress channels, two slot workloads and (after P32a) two run records. P28b and P32b each carry a cancel/orphan test.
14. **Skills.** The in-repo skills are upload-only (`npm/skills/source/er-review/SKILL.md:55`, `er-respond/SKILL.md:118`). P30b.2 keeps bare uploads working (`tour.json` test), or the installed skills break.
15. **Card AI.** The command blocks on the reply; an undrained progress channel in `run_blocking` can deadlock. P27's test pins the drain.
16. **Model discovery** spawns agent CLIs with no slot; ADR 0021 allows it, and the exit criterion says so.
17. **The OS-launch carve-outs are whole files.** A1's E0710 rules out a per-site `#[expect(er::agent_spawn)]`; the permanent file list is small and reviewed, and a spawn added to one is a visible change.
18. **A7's config move.** The nine tests and their plants are the proof that no setting is dropped or rewritten: field-for-field migration, the unparseable file left alone with no `agents.toml`, a crash between the writes completing on restart, no overwrite after a failed load, a second run changing nothing, `.invalid` backups that do not collide, an unparseable `agents.toml` refused, a read-back mismatch leaving `config.toml` alone, and both files loading `agents.toml`'s values. The parse-failure overwrite also exists in shipped v0.6; the `main` fix (#460) reached release through #467, and P37 extends it to the split.

## Open questions: all closed

- **`agents.toml`:** D16; store root, catalog compiled in, implemented by P37.
- **Panel measurement:** D17; free `quorum()` proxy, no extra arbiter call, default unchanged.
- **`[commands].summary`:** D18; retired key with a first-launch notice, summary as a built-in job.
- **Crash resume:** coordinator; mark `Interrupted`, offer a re-run of unfinished stages, no auto-resume, no spawn on load.
- **Registry:** coordinator; P36 as written.
- **D1:** settled by ADDENDUM-01.

## Re-checked at `76d89a66f`

- **P20:** `arena/seeded.rs:130` reads `review.json` through `read_sidecar`; the projection (`ai/projection.rs:29`–`:58`) owns the lens backfill, the expert and professor merges, the provenance restore, validation and the arbiter overlay, and has eight goldens under `crates/er-engine/tests/golden/findings_projection/`. P34a extends those goldens. The arena's seeded dedupe keeps its own merge for arbitration, and its expert filter already matches the projection's (`arena/seeded.rs:29`–`:33`), which P33a pins with the existing tests.
- **P21:** the prompts shifted (expert `:409`, `:588`, `:624`; tour `:683`; professor `:983`; triage `:1248`; questions `:1422`, `:1774`; github-comments `:1585`; summary `:1625`, `:1706`; notes `:1842`, `:1907`). The two review rewrites moved to the validate prompt (`:1466`), which writes `validation.json`. That kind is Bare (`ai/validation.rs:80`) and registered with the three scoped kinds (`store_registry.rs:47`–`:50`). The flow gained a `Verifying` stage (`ai/review_flow.rs:17`–`:21`, `:122`–`:128`), now part of P32a's stage list. `ai/overlay_files.rs` is the deliberate raw helper for the non-registered `.prev.json` and `review-files.txt` files, so it stays outside the deny list.
- **P24:** uploads commit through the store (`sidecar_upload.rs:235`–`:273`, `:458`–`:459`); `validate_contents_before_write` (`:180`–`:229`) is the validator P30a reuses; `sidecar_upload.rs` is already on `sidecar-fs-deny`; a bare `tour.json` input still commits because the upload path parses it and the store writes the artifact's mode, which becomes the envelope after P30b.
- **#441:** the tests P26a moves (`agent_run`, `agent_slots`, `agent_timing`) carry no `set_var`/`remove_var`; they read the environment through `crate::env_var::var` and use temp dirs or `with_test_storage_root`. `managed_config_path` reads `ER_CONFIG_PATH` through `crate::env_var::var` (`config/mod.rs:1676`–`:1687`), which P31 relies on.
- **#467:** merged as `21591290c`; the release tip is `76d89a66f`. `config/mod.rs` and `app/state/mod.rs` line numbers are refreshed against it. The #460 guard is now on release: `CONFIG_LOAD_BANNER` and the failed-load set (`config/mod.rs:1691`–`:1760`), with `save_config_to` (`:1939`–`:1955`) refusing while a path is recorded and re-parsing before writing.
- **#464 (open):** sidecar writers read-modify-write outside the store lock, on the scoped overlay reply/resolve/edit/stamp paths, the validation outcome drop, and the base `review.json` writers. P30a.2's patch contract covers the update kinds and P30b/P34b delete the review rewrites, but the scoped overlay paths (`ai/scoped_overlay.rs`) sit in no phase-3 entry. Coordinator decision: #464 (D4, in progress) lands before P30a starts, and P30a depends on it.
- **#468 (open):** the ETXTBSY test flake. It touches the `comments.rs` tests, `app/state/mod.rs`, `env_path.rs:191` and `tests/arena_timing.rs:84`, and `arena/adapter.rs`'s real-process test. P26a, P28a, P29 and P33a move or delete those tests; coordinator decision: #468 (T2, PR #469) lands before P26a, and P26a depends on it.

