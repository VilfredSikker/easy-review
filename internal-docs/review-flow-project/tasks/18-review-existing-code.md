# 18 — Review existing code outside a PR

Status: needs-design
Idea: 17 in the original idea list (summarised in ../README.md)
Depends on: 06-pre-existing-tag.md, 16-teaching-pass-scope.md
Impact: L · Complexity: M · Size: M
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — er only reviews diffs. Old bugs in code nobody is changing stay hidden until a PR happens to touch them, and then they show up in that PR's review as if the author caused them.
- **Why it matters** — the blog runs reviewers over existing code every night "to catch old bugs before they show up somewhere else". It also gives the convergence work in task 08 somewhere to send old bugs other than the current PR.
- **The fix** — a review whose subject is a path or module as it is now, proposed name **Sweep**. A person picks paths, an agent reviews that code, and the findings are listed apart from any PR, all marked Pre-existing. It reuses the subject and staleness mechanism from task 16. Running it on a schedule is left to tools outside er; this task says how, and builds that part only if the owner asks.
- **TL;DR** — let er review a folder of unchanged code, with every finding marked Pre-existing.

## Source
From *How the Claude Code team does code review*, https://claude.dev/blog/solving-code-review-at-anthropic/ ("Review has to converge"):

> Review agents also run nightly over existing code, helping us catch old bugs before they show up somewhere else.

And on Pre-existing findings (README "Sources"): a real bug the PR did not introduce, "so a bug the PR didn't introduce doesn't look like the author's fault."

## Mission & definition of done
Add a review over unchanged code, stored per repo and outside any view bucket, whose findings are Pre-existing by definition. Step 1 is a design step (ADR plus `CONTEXT.md` changes) that a person must approve. Scheduling is a separate, optional step that stops for an owner decision.

- [ ] Step 1: ADR and `CONTEXT.md` changes written, `just docs-check` passes, owner approval recorded under `## Comments`.
- [ ] A Sweep over one or more repo paths can be started from the TUI AI Hub and the desktop; the desktop lists Sweeps and their findings.
- [ ] Its agent runs read-only on every CLI family and takes an agent slot; the host writes the result.
- [ ] Every finding in a Sweep has task 06's `pre_existing: true` and an anchor on a whole-file line (`hunk_index: None`, `line_start: Some`).
- [ ] A Sweep is stale when any reviewed path's committed content changed (task 16's rule); stale Sweeps offer Rerun, nothing reruns on its own.
- [ ] Sweep findings never appear in a diff review's finding list, its counts, or a round count (task 08).
- [ ] Diff review prompts are byte-for-byte unchanged (Step 4's guard test).

**Risk:** ordinary. Reuses task 16's spawn and store pattern; the one change to shared code is lifting a prompt block into a constant, guarded by a byte-equality test.

## Before you start (orientation)
- **Branch:** see `../README.md` "Branching". Feature work: current release branch.
- **Prerequisites:** task 06 and task 16 are `Status: done`. Check both files' `Status:` lines first. If either is not done, stop.
- **Commands:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine <filter>`
  - `~/.claude/scripts/locked.sh . cargo test -p er-tui <filter>`
  - `~/.claude/scripts/locked.sh . just lint-ui`, `~/.claude/scripts/locked.sh . just test-ui`
  - `~/.claude/scripts/locked.sh . just clippy`, `just docs-check`, `just comments-check`
- **The map** (confirmed at c96dc0f12 unless marked as coming from task 16):
  - From task 16 (not in the tree at c96dc0f12), as 16-teaching-pass-scope.md names them: in `crates/er-engine/src/ai/lesson.rs`, `head_commit(repo_root)` and `object_id_at(repo_root, commit, path)` and `mark_stale(lessons, repo_root)` (16 Step 3); the path check, an unnamed five-line copy of `validate_path` from `crates/er-engine/src/app/state/preview.rs:374` (16 Step 2); `persist_lesson_from_agent_stdout(stdout, stream_json, pinned: LessonPins, repo_root, output_path)` and its marker extraction and atomic write (16 Step 2). In `crates/er-engine/src/app/state/background.rs`, `HostWriteLesson { output_path, pins, repo_root }` and the field `PendingBackgroundTask.host_write_lesson` (16 Step 4.1). In `comments.rs`, `fn claude_allowed_tools(read_only, prepared_diff)` (16 Step 4.2) and `App::spawn_background_lesson(question, paths)` (16 Step 4.4). In the TUI, `InputMode::LessonPrompt` (16 Step 7). 16 allows the owner to rename "Lesson" in its Step 1; if that happened, use the names recorded under `## Comments` in 16 and check with `grep -rn "fn object_id_at\|HostWriteLesson" crates/er-engine/src`.
  - `crates/er-engine/src/ai/review.rs:271` `pub struct Finding` — `hunk_index: Option<usize>` (`:295`), `line_start: Option<usize>` (`:297`), `line_content: String` (`:304`), `outside_diff: bool` (`:318`). `:120` `pub fn lenient_findings`.
  - The Pre-existing flag: `Finding.pre_existing: bool` (`#[serde(default)]`), added in `crates/er-engine/src/ai/review.rs` by 06-pre-existing-tag.md Step 2. Set it to `true` on every Sweep finding. `outside_diff` is a separate field: after task 06 it means "the anchor sits on a line the change did not add or remove", which says nothing about a Sweep. Leave it `false` (06's ADR names task 18 as the reason `pre_existing` is a field of its own).
  - `crates/er-engine/src/ai/prompts.rs:83` `fn review_rules_preamble_with_hash(...)`. Its `### Severity (P0 / P1 / P2)` block starts at `:146`; `### Confidence and verification` follows; `### Scope` starts at `:159`. Its "Annotate and anchor" block (`:140–144`) is diff-only and must not reach a Sweep prompt.
  - `crates/er-engine/src/ai/experts.rs:10` `pub struct FindingCaps { per_file: u32, total: u32, is_expert: bool }` at c96dc0f12. Task 06 Step 3 adds `pre_existing: u32` (and task 02 adds a `FindingCaps::professor()` constructor), so after 06 every struct literal needs `pre_existing: …`.
  - `crates/er-engine/src/app/state/comments.rs:3045` `fn spawn_background_agent_task(...)`; `:3240` `let readonly_run = …`; `:3396` working-directory choice (`managed_local` → `repo_root`).
  - TUI hub: `crates/er-engine/src/app/state/mod.rs:526` (`enum HubAction`), `:6808` (`HubItem` for importance); dispatch `crates/er-tui/src/input/mod.rs:282`.
  - Desktop: `crates/er-desktop/src/commands.rs:4018` `run_importance_agent`; `crates/er-desktop/src/main.rs:1913` registration.
  - Headless CLI: `crates/er-tui/src/main.rs:67` `enum Commands { Uninstall {…}, Update }`; `Update` returns before any terminal setup (`main.rs:187`, `return update::run();`).
  - er-mcp: `crates/er-mcp/src/server.rs` tools `pr_prepare` (`:822`) and `pr_upload` (`:901`) — the existing route where an outside agent is the reviewer and er only stores what it uploads. Upload kinds are `enum SidecarKind { Triage, Review, Tour }` (`crates/er-engine/src/sidecar_upload.rs:18`). er-mcp builds er-engine with `default-features = false` (`crates/er-mcp/Cargo.toml:15`), so it cannot call anything under `app` or `arena`.
- **Conventions to follow** — the review rules wording a Sweep must keep, from `crates/er-engine/src/ai/prompts.rs:146`:

  ```text
  ### Severity (P0 / P1 / P2)
  - P0 → `high` — must fix (security, data loss, broken contract)
  - P1 → `medium` — should fix (logic gap, missing edge case, shallow test)
  ```

  and the document anchor shape from ADR 0041: a line outside any hunk is `hunk_index: None` with `line_start: Some`, `line_content` and context taken from the file's full text.
- **Mental model** — A diff review reads a prepared diff and anchors each finding on a `+` or `-` line by `[h<N> L<M>]` tags. A Sweep has no diff: the agent reads files directly, so a finding anchors on a file line, the way a document-anchored question does (ADR 0041). Because nothing in a Sweep was introduced by a PR, every finding is Pre-existing (task 06). Like a Lesson (task 16), a Sweep belongs to the repo, is keyed by the commit and the object ids of what it read, and goes stale when those change.
- **ADRs to read first:**
  - 0003, 0004 — managed storage and view buckets; a Sweep sits outside the buckets, beside task 16's Lessons.
  - 0009 — findings are AI-owned; a Sweep's findings follow the same rule.
  - 0010 — staleness kinds; a Sweep uses task 16's object-id rule and stays out of `is_stale`.
  - 0021 — the spawn takes a slot. A separate `er` process (a scheduled CLI run) has its own slot pool and is not capped by an open TUI's slots.
  - 0022 — read-only agent, host writes.
  - 0023 — prompts compiled in.
  - 0041 — document anchors.
  - Task 16's new ADR (number assigned there) — the Lesson store and staleness rule this task reuses.

## Scope boundaries
- **In scope:** ADR and `CONTEXT.md`; moving task 16's subject and staleness helpers into a shared module; Sweep type, store, loader, staleness; Sweep prompt; spawn; TUI entry; desktop entry plus a findings list; the scheduling write-up; the scheduling build only if the owner approves in Step 8.
- **OUT of scope / DO NOT TOUCH:**
  - Merging Sweep findings into any diff review, `review.json`, or a PR's findings.
  - Task 08's round counting and task 03's outcome records, except to confirm Sweep findings do not feed them.
  - Posting Sweep findings to GitHub. A Sweep has no PR; GitHub accepts comments on diff lines only (ADR 0041 Consequences).
  - A daemon, timer, or background schedule inside er.
  - Preview's refusal of files outside the review (`crates/er-engine/src/app/state/preview.rs:271`).

## Steps
### Step 1 — ADR and CONTEXT.md, then stop
- **Goal:** owner approves the design.
- **Why / invariant:** this widens what a Finding is. `CONTEXT.md` today defines **Finding** as "A remark an AI review produced about a specific place in the diff."
- **Files:** new `docs/adr/<next>-sweeps-review-code-outside-a-diff.md` (next free number after task 16's); `CONTEXT.md`.
- **Do this:**
  1. `CONTEXT.md`: add **Sweep** after **Triage**: "A review of code as it stands, with no diff: its subject is a set of repo paths. Every finding it produces is Pre-existing. It belongs to the repo and lives outside every view bucket. _Avoid_: scan, audit, nightly review." (`scan` is already an *Avoid* word for **Triage**; do not use it as a name.) Amend **Finding** to "…about a specific place in the diff, or, for a Sweep, in a file." Amend **Lens** to list `sweep` beside `general`, `professor` and `arbiter`, because Step 3 sets that value.
  2. ADR, short, each decision with its reason:
     - Store: `<storage_root>/repos/<slug_repo(repo_root)>/sweeps/<id>/sweep.json`, the same layout and local-clone rule as task 16's Lessons.
     - Staleness: task 16's rule (commit plus per-path object ids), kept out of `is_stale`.
     - Anchors: document-style (`hunk_index: None`, `line_start: Some`, `line_content` from the file). No relocation; a stale Sweep is rerun.
     - Pre-existing by definition: the host sets task 06's `pre_existing: true` on every finding, whatever the agent wrote.
     - Write path: read-only agent, host writes (ADR 0022). Diff reviews still let the agent write `review.json`; a Sweep does not, because its subject comes from user input and the host already has the parse-validate-write path from task 16.
     - Kept apart: Sweep findings never merge into a diff review, never count toward rounds or "all answered".
     - Scheduling lives outside er (see Step 8).
  3. `just docs-check`.
  4. Note under `## Comments` here: ADR path, terms, "awaiting owner approval". Stop.
- **Verify:** `just docs-check` → exit 0; `grep -n "^\*\*Sweep\*\*" CONTEXT.md` → one match.
- **If it fails:** fix the docs only.

**STOP here until the owner approves in writing.**

### Step 2 — Share task 16's subject and staleness helpers
- **Goal:** one module both Lessons and Sweeps use for the commit, object ids, path checks and the stale test.
- **Why / invariant:** two copies of the staleness rule would drift; Lesson behaviour must not change.
- **Files:** new `crates/er-engine/src/ai/subject.rs`; `crates/er-engine/src/ai/lesson.rs`; `crates/er-engine/src/ai/mod.rs`.
- **Do this:** move (do not rewrite) `head_commit`, `object_id_at`, the path check, the per-path object-id capture and the comparison inside `mark_stale` into `subject.rs` as `pub fn` items. `lesson.rs` calls them. Move the tests that cover those functions with them; leave Lesson-specific tests in place.
- **Watch out for:** `ai` is always compiled; `subject.rs` must not import `crate::app` or `crate::arena`.
- **Verify:** record the Lesson test count first: `~/.claude/scripts/locked.sh . cargo test -p er-engine lesson` → note N. After the move, `~/.claude/scripts/locked.sh . cargo test -p er-engine lesson` plus `~/.claude/scripts/locked.sh . cargo test -p er-engine subject::` → the two counts add up to N, all pass.
- **If it fails:** revert the move and stop; do not change task 16's behaviour.

### Step 3 — Sweep type, store, persist, load
- **Goal:** `crates/er-engine/src/ai/sweep.rs`.
- **Why / invariant:** the host pins every field it knows, and every finding is Pre-existing.
- **Do this:**
  ```rust
  pub const SWEEP_JSON_BEGIN: &str = "---SWEEP_JSON---";
  pub const SWEEP_JSON_END: &str = "---END_SWEEP_JSON---";

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct Sweep {
      pub version: u32,
      #[serde(default)] pub id: String,
      #[serde(default)] pub created_at: String,
      #[serde(default)] pub paths: Vec<String>,                 // pinned
      #[serde(default)] pub focus: String,                      // pinned, optional user text
      #[serde(default)] pub commit: String,                     // pinned
      #[serde(default)] pub sources: BTreeMap<String, String>,  // host-computed
      #[serde(default)] pub dirty_at_write: bool,
      #[serde(default)] pub files: HashMap<String, SweepFile>,
      #[serde(skip)] pub stale: bool,
  }
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct SweepFile {
      #[serde(default, deserialize_with = "super::review::lenient_findings")]
      pub findings: Vec<Finding>,
  }
  ```
  `persist_sweep_from_agent_stdout(...)` mirrors task 16's Lesson persist, plus, for each finding: set `hunk_index = None`; drop it if `line_start` is `None` or past the file's line count at `commit`; refill `line_content` from the file at `commit` (`git show <commit>:<path>`), never trusting the agent's copy; set `lens` to `"sweep"` (CONTEXT.md **Lens**); set `pre_existing = true` (task 06's field) and leave `outside_diff = false`; prefix ids `sweep-` (copy `prefix_finding_id`, `crates/er-engine/src/ai/professor.rs:50`); drop findings for files not under the Sweep's `paths`; apply caps (3 per file, 15 total) after filtering. `load_sweeps(repo_root)` and `mark_stale` reuse `subject.rs`.
- **Watch out for:** `lens` is set by the merge path, never by the producer (`review.rs:274–277` doc comment).
- **Verify:** tests in `sweep.rs`:
  - `every_persisted_finding_is_pre_existing` (agent omits `pre_existing` → file has `true`);
  - `persist_refills_line_content_from_the_commit` (agent sends wrong `line_content`);
  - `persist_drops_findings_outside_the_subject_paths`;
  - `persist_forces_document_anchors` (agent sends `hunk_index: 2` → stored `None`).
  `~/.claude/scripts/locked.sh . cargo test -p er-engine sweep::` → 4 passed. Plant a break (skip the `pre_existing` assignment), rerun, see `every_persisted_finding_is_pre_existing` fail, revert in the same command.
- **If it fails:** fix `sweep.rs` only.

### Step 4 — Sweep prompt, sharing the severity rules without changing diff prompts
- **Goal:** `pub fn build_sweep_prompt(repo_root: &str, paths: &[String], focus: Option<&str>) -> String` in `crates/er-engine/src/ai/prompts.rs`.
- **Why / invariant:** a Sweep must grade findings by the same severity and do-not-flag rules as a diff review, and diff review prompts must not change by one byte.
- **Do this:**
  1. Before any edit, add a guard test `review_rules_preamble_is_unchanged_by_the_sweep_split` to `mod tests` (`prompts.rs:1923`) that compares `review_rules_preamble(...)` output for two fixed inputs (prepared diff true and false, `FindingCaps { per_file: 3, total: 10, is_expert: false, pre_existing: 0 }`; the `pre_existing` field exists once task 06 has landed, which this task requires) against string constants you paste from a run on the unmodified code. Run it, see it pass.
  2. Lift the text from `### Severity (P0 / P1 / P2)` through the end of `### Confidence and verification` (`prompts.rs:146–157`) into `const SEVERITY_AND_CONFIDENCE_RULES: &str` and splice it back into `review_rules_preamble_with_hash`'s `format!`. Rerun the guard test: it must still pass unchanged.
  3. Build the Sweep prompt with `push_str` (pattern: `build_importance_prompt`, `prompts.rs:961`): role (review the listed paths as they are now; every finding is a pre-existing bug), work in `repo_root`, stay inside the repo (first bullet of `### Scope`, `:160`), `SEVERITY_AND_CONFIDENCE_RULES`, anchors ("cite `path` and the 1-based `line_start` of the file as it is in the checkout; there is no diff"), caps, and the output block between `SWEEP_JSON_BEGIN` / `SWEEP_JSON_END` with "do **not** use Write, Edit, or a shell redirect". Add `## Focus (user-provided)` only when `focus` is set.
- **Watch out for:** never include `diff-tmp`, `diff-annotated`, `[h<N>`, or "`+` or `-` lines".
- **Verify:** tests `sweep_prompt_has_no_diff_rules`, `sweep_prompt_shares_the_severity_rules` (contains `SEVERITY_AND_CONFIDENCE_RULES`), plus the guard. `~/.claude/scripts/locked.sh . cargo test -p er-engine prompts::` → all pass, count = previous + 3.
- **If it fails:** if the guard test fails after the lift, undo the lift and copy the block into the Sweep builder instead, with a test that the two copies are equal. Record that under `## Comments`.

### Step 5 — Spawn
- **Goal:** `App::spawn_background_sweep(&mut self, paths: Vec<String>, focus: Option<String>) -> Result<()>`.
- **Why / invariant:** read-only on every family (ADR 0022), takes a slot (ADR 0021), runs in the repo.
- **Files:** `crates/er-engine/src/app/state/comments.rs`, `crates/er-engine/src/app/state/background.rs`.
- **Do this:** follow task 16's `spawn_background_lesson` exactly: refuse remote-only tabs and empty `paths`; `managed_local: true`; `scope: format!("sweep:{id}")`; kind `sweep`. Extend task 16's host-write parameter so a Sweep is read-only at every place task 16 Step 4 named: the field on `PendingBackgroundTask` (`crates/er-engine/src/app/state/background.rs:130`), `launch_background_agent_task` (`comments.rs:3115`), the `readonly_run` decision (`:3240`), and the Claude tool list (`:3333`, which task 16 lifted into `claude_allowed_tools`). If task 16 added a single-purpose `Option<HostWriteLesson>`, turn it into an enum with `Lesson` and `Sweep` variants rather than adding another parameter or field.
- **Watch out for:** the worker's completion branch for a Sweep must call `persist_sweep_from_agent_stdout`, and a failure must surface like the diagram branch (`comments.rs:3539`, `.with_context(...)`).
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine` → full engine suite passes. Manual check in Step 7.
- **If it fails:** stop.

### Step 6 — TUI entry
- **Goal:** AI Hub item "Review existing code…" that asks for paths, starts the Sweep, and notifies on completion with the finding count. Reading Sweep findings in the TUI is a follow-up, matching task 16's TUI scope.
- **Files:** `crates/er-engine/src/app/state/mod.rs:526` (`HubAction::RunSweep`), `:6808` (`HubItem`), `:327` (`enum InputMode`, add `SweepPaths`); `crates/er-tui/src/input/mod.rs:282` (dispatch). Follow task 16 Step 7, which adds `InputMode::LessonPrompt` (in `crates/er-engine/src/app/state/mod.rs:327`) modelled on `InputMode::BaseBranch` (`crates/er-tui/src/input/mod.rs:699`, `handle_base_branch_input`), with key routing in `crates/er-tui/src/main.rs:482` and the prompt line in `crates/er-tui/src/ui/status_bar.rs:803` and `:1000`. Copy that shape for `SweepPaths`.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-tui` → passes. Manual: `just run`, start a Sweep on `crates/er-engine/src/storage.rs`, see the notification and the file under `<storage_root>/repos/<repo>/sweeps/`.
- **If it fails:** stop and ask before designing new TUI layout.

### Step 7 — Desktop command and card
- **Goal:** `run_sweep(paths, focus)` and `delete_sweep(id)` commands; snapshot field; a card listing Sweeps and findings.
- **Files:** `crates/er-desktop/src/commands.rs` (copy `run_importance_agent`, `:4018`); `crates/er-desktop/src/main.rs:1913` (register); `crates/er-desktop/src/snapshot.rs` (new explicit field, ADR 0006); a card beside task 16's Lessons card in `desktop-ui/src/lib/components/RightPanel.svelte` (the diagrams card mounts at `:416`). Show findings with the same Pre-existing styling task 06 introduced (06 Step 6.5): a `Pre-existing` pill with the classes `px-1 py-0.5 rounded border border-hairline text-[9px] text-muted`, as in `desktop-ui/src/lib/components/InlineFinding.svelte`. Every Sweep finding gets it. Reuse 06's markup; add no new theme token.
- **Verify:** `~/.claude/scripts/locked.sh . just lint-ui` → exit 0; `~/.claude/scripts/locked.sh . just test-ui` → pass. Manual: run a Sweep, commit a change to one swept file, Sweep shows stale with Rerun; commit to an unrelated file, Sweep stays fresh.
- **If it fails:** stop after three attempts and report.

### Step 8 — Scheduling: write up the options, then stop
- **Goal:** the owner chooses how (or whether) a Sweep runs on a schedule. er is not a daemon and will not become one.
- **Do this:** append under `## Comments` a short proposal with these two options, then stop:
  - **Option A — er-mcp (no new er process type).** Add `repo_sweep_prepare` / `repo_sweep_upload` tools to `crates/er-mcp/src/server.rs`, mirroring `pr_prepare` / `pr_upload`: prepare returns the paths, commit and the Sweep prompt text; the calling agent reviews and uploads JSON; er-mcp validates with `persist_sweep_from_agent_stdout`'s validation (which lives in `ai`, always compiled, so er-mcp can call it). A user schedules it with cron or launchd: `claude -p "Use the easy-review MCP repo_sweep tools to sweep crates/er-engine/src/git"`. The calling agent is the reviewer, as with every er-mcp tool, so er spawns nothing.
  - **Option B — `er sweep <paths…>` subcommand.** Add a variant to `enum Commands` (`crates/er-tui/src/main.rs:67`) that returns before terminal setup, like `Update`. It needs a headless way to run `spawn_background_sweep` and wait for its result channel without the TUI event loop; that does not exist today. A cron-launched `er` has its own agent-slot pool (ADR 0021), separate from any open TUI.
  - Recommendation: Option A, because it needs no headless `App` and matches er-mcp's existing prepare/upload split.
- **Verify:** none; this is a decision request.
- **If it fails:** n/a.

**STOP here. Build neither option without the owner's choice.**

## Gotchas & landmines
- **`outside_diff` is not the Pre-existing flag.** Task 06 keeps them as two fields; set `pre_existing`, leave `outside_diff` false.
- **No relocation.** A Sweep finding's line is fixed at `commit`. Do not run Sweep findings through `relocate.rs`; it works on hunks and would mark every one lost.
- **The importance agent is not read-only** (`spawn_background_importance` passes no host-write target, so `readonly_run` is false). Do not copy it; see task 16's Gotchas.
- **Dedup key ignores `er_dir`** (`BackgroundTaskTarget::key`, `crates/er-engine/src/app/state/background.rs:34`). Put the Sweep id in `scope`.
- **Working directory** follows `managed_local` (`comments.rs:3396`). Set it, or the agent starts in the Sweep directory and reads nothing.
- **er-mcp has no `app` or `arena`.** Anything Option A needs must live in `ai/`.

## Decision log (forks pre-answered)
- **If you want to put Sweep findings into `review.json` so existing UI shows them:** don't. They would count as the PR's findings, the opposite of what the `pre_existing` flag is for.
- **If the agent anchors a finding on a line that does not exist at `commit`:** drop that finding.
- **If a subject path is a directory:** keep it as one `sources` entry (a tree object id); any change under it marks the Sweep stale. Accept the coarse staleness.
- **If the owner wants Sweep findings handed to a fixing agent:** that is task 09's fix action; stop and ask whether to extend it.
- **If task 06 caps Pre-existing findings per review:** the Sweep caps in Step 3 replace that cap for Sweeps, because every Sweep finding is Pre-existing and the per-review cap would empty it. Record this in the ADR.
- **Name:** "Sweep". If the owner picks another name in Step 1, use it everywhere, including the kind string and `lens` value.
- **Consistency pass (2026-10-17):** task 16's names and task 06's field and pill styling are filled in from those files. On a Sweep finding, `outside_diff` stays `false`: 06 gives it the meaning "anchor on a line the change did not touch", and a Sweep has no change. Task 06's per-review Pre-existing cap does not apply to Sweeps (the decision on caps further down this log covers it).

## Full verification
1. `~/.claude/scripts/locked.sh . cargo test -p er-engine sweep` → new tests pass.
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine prompts::` → passes, guard test included.
3. `~/.claude/scripts/locked.sh . cargo test -p er-engine lesson` and `subject::` → task 16's tests all still pass.
4. `~/.claude/scripts/locked.sh . cargo test -p er-engine` → full suite passes.
5. `~/.claude/scripts/locked.sh . cargo test -p er-tui` → passes.
6. `~/.claude/scripts/locked.sh . just build-engine-headless` → builds.
7. `~/.claude/scripts/locked.sh . just clippy` → exit 0; `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
8. `just docs-check` → exit 0; `just comments-check` → exit 0.

## Rollback
Revert the commits. Sweep data lives only under `<storage_root>/repos/<repo>/sweeps/`. If Step 2's helper move must be undone alone, revert that commit; Lessons keep working because the move changed no behaviour.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- Task 06 or task 16 is not done.
- After Step 1 until approved; at Step 8 always.
- Task 06's tag is tied to diff anchors in a way a document anchor cannot satisfy.
- The owner wants Sweep findings to feed task 03 outcomes or task 19 metrics: decide where those records live first.

## Comments
