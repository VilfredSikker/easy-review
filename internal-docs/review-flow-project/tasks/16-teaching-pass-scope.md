# 16 — Teaching pass for a path or a question

Status: needs-design
Idea: 9 in the original idea list (summarised in ../README.md)
Depends on: —
Impact: M · Complexity: M · Size: M
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — er can only teach about a diff. Professor (teaching text) and diagrams (Mermaid pictures) both read the prepared diff and nothing else, and Preview refuses to open a file that is not in the review. A person about to change a module they have never read has no way to ask er "how does this part work?".
- **Why it matters** — the blog's point: when machines do most of the reading, people still need to learn the code, and an explanation of the module you are about to change teaches more than reading trivial diffs line by line.
- **The fix** — a new kind of artifact, proposed name **Lesson**: the person names some paths, or asks a question ("How does er load config?"), and an agent writes a short teaching text, with insights pointing at file lines and an optional diagram. It is kept per repo, records which code it read, and is marked stale when that code changes. An optional global setting says how the person likes to be taught.
- **TL;DR** — let professor and diagrams teach a path or answer a question, instead of only explaining a diff.

## Source
From *How the Claude Code team does code review*, https://claude.dev/blog/solving-code-review-at-anthropic/ ("Sharing context across the team"):

> When someone wants to understand a part of the codebase, they ask Claude to write an explainer for exactly that slice. It takes minutes and reflects the code as it is today.

> Some of us go a step further and have Claude build a small artifact that visualizes that part of the code and teaches it the way that person likes to learn.

> A one-line copy change teaches you almost nothing. An explainer for the module you're about to change teaches you a lot.

## Mission & definition of done
Give er a teaching artifact whose subject is a set of repo paths, a free-text question, or both, instead of a diff. Step 1 is a design step: an ADR and a `CONTEXT.md` entry that a person must approve. Steps 2–8 build the engine side and both front ends only after that approval.

- [ ] Step 1: an ADR under `docs/adr/` (next free number) and `CONTEXT.md` entries for the new terms exist, `just docs-check` passes, and the owner has approved them in writing (a dated line under `## Comments` in this file).
- [ ] A Lesson can be generated from the TUI AI Hub and from the desktop, for a path set, a question, or both. The desktop lists and shows Lessons; the TUI starts them and reports completion.
- [ ] The agent that writes a Lesson runs read-only on every CLI family; the host parses its stdout and writes the file.
- [ ] A Lesson is stored outside every view bucket, under the repo, and survives switching branch or diff mode.
- [ ] A Lesson whose source files changed since it was written is shown as stale, with a Regenerate action. Nothing regenerates on its own.
- [ ] An optional `[learning]` section in global config is appended to the Lesson prompt when set, and changes nothing when absent.
- [ ] Professor, diagram and tour behaviour on diffs is unchanged (their existing tests pass untouched).

**Risk:** ordinary. New artifact kind and a new agent spawn; the one sharp edge is keeping the agent read-only (Step 4).

## Before you start (orientation)
- **Branch:** see `../README.md` "Branching". This is a feature: use the current release branch.
- **Commands:**
  - Engine tests: `~/.claude/scripts/locked.sh . cargo test -p er-engine <filter>`
  - TUI tests: `~/.claude/scripts/locked.sh . cargo test -p er-tui <filter>`
  - Desktop UI: `~/.claude/scripts/locked.sh . just lint-ui` and `~/.claude/scripts/locked.sh . just test-ui`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `just docs-check`, `just comments-check`
- **The map** (all confirmed at c96dc0f12):
  - `crates/er-engine/src/ai/professor.rs` — `ProfessorReview` sidecar (`diff_hash`, `summary`, `files` of `Finding`s). Model for the Lesson's teaching content. Do not change it.
  - `crates/er-engine/src/ai/prompts.rs:838` `fn professor_rules_preamble(...)`, `:868` `fn professor_lens_instructions(user_focus: Option<&str>) -> String`, `:894` `fn professor_output_section(output_dir: &str) -> String`, `:1026` `pub fn build_professor_review_prompt_local_managed(...)`, `:1065` `pub fn build_professor_review_prompt_prepared_diff(...)`. Read for tone; do not reuse the preamble (see Gotchas).
  - `crates/er-engine/src/ai/prompts.rs:681` `pub fn build_diagram_prompt_prepared_diff(scope: &str, output_dir: &str, output_file: &str, diff_hash: &str, kind: &str, custom_prompt: Option<&str>) -> String` — its "Mermaid rules" block is the part to copy into the Lesson prompt.
  - `crates/er-engine/src/ai/prompts.rs:961` `pub fn build_importance_prompt(repo: &str, repo_root: &str) -> String` — the one existing prompt whose subject is the repo, not a diff. Copy its shape.
  - `crates/er-engine/src/ai/diagrams.rs:155` `pub fn persist_diagram_from_agent_stdout(stdout: &str, stream_json: bool, expected_kind: &str, expected_diff_hash: &str, custom_prompt: Option<&str>, output_path: &Path) -> Result<()>` and its private helpers `extract_diagram_json_payload` (`:267`), `extract_agent_stdout_text` (`:315`), `write_diagram_atomic` (`:250`). The parse-validate-write pattern to copy.
  - `crates/er-engine/src/app/state/comments.rs:2869` `pub fn spawn_background_importance(&mut self) -> Result<()>` — spawn with no diff; refuses remote-only tabs.
  - `crates/er-engine/src/app/state/comments.rs:3045` `fn spawn_background_agent_task(&mut self, kind: String, command_name: &str, target: BackgroundTaskTarget, prompt: String, prepared_diff: bool, host_write_diagram: Option<HostWriteDiagram>) -> Result<()>` — the one spawn path; it acquires an agent slot (`agent_slots::acquire_blocking` at about `:3306`).
  - `crates/er-engine/src/app/state/comments.rs:3240` `let readonly_run = host_write_diagram.is_some();` — read-only enforcement is keyed on the diagram write target today.
  - `crates/er-engine/src/app/state/comments.rs:3521` — the importance branch of the worker's completion code; `:3539` the diagram host-write branch. A Lesson host-write branch goes beside them.
  - `crates/er-engine/src/app/state/background.rs:19` `pub struct BackgroundTaskTarget`, `:146` `pub struct HostWriteDiagram`.
  - `crates/er-engine/src/storage.rs:14` `pub fn storage_root() -> PathBuf`, `:63` `pub fn slug_repo(repo_root: &str) -> String`, `:136` `pub fn view_bucket_dir(...)`.
  - `crates/er-engine/src/config/mod.rs:24` `pub struct ErConfig` (sections are `#[serde(default)]` fields), `:541` `pub struct SummaryConfig` (shape of a small section).
  - `crates/er-engine/src/app/state/preview.rs:271` `fn preview_file_status(&self, path: &str) -> Result<FileStatus>` — returns `"File is no longer in this review"` for any path not in the diff. Left alone by this task (see Scope).
  - `crates/er-engine/src/app/state/mod.rs:526` `RunImportanceAgent` in `enum HubAction`; `:6808` its `HubItem`. TUI dispatch: `crates/er-tui/src/input/mod.rs:282`.
  - Desktop: `crates/er-desktop/src/commands.rs:4018` `pub async fn run_importance_agent(...)`, `:4037` `pub async fn generate_diagram(...)`; registration list `crates/er-desktop/src/main.rs:1913`; UI card `desktop-ui/src/lib/components/DiagramsCard.svelte`.
- **Conventions to follow** — the host-written, read-only agent output. From `crates/er-engine/src/ai/diagrams.rs:217`:

  ```rust
  // Never trust the agent for kind / hash / custom prompt — pin from the
  // harness so a prompt-injected payload cannot retarget the sidecar.
  diagram.kind = expected_kind.to_string();
  ```

  and the spawn with no diff, from `crates/er-engine/src/app/state/comments.rs:2874`:

  ```rust
  if self.tab().is_remote() {
      anyhow::bail!("Ranking importance needs a local clone of this repo");
  }
  ```
- **Mental model** — An AI action builds a prompt from Rust source (ADR 0023), queues a background task on `App`, and a worker thread runs the agent CLI once it holds an agent slot. Most agents write their own sidecar into the tab's view bucket; diagrams instead print JSON between markers and the host validates and writes the file, because the prompt carries untrusted content (ADR 0022). Every sidecar today is identified by the SHA-256 of the diff it was made from, and goes stale when that hash moves (ADR 0010). A Lesson has no diff, so it needs a different identity and a different staleness test, and it cannot live in a view bucket, whose whole purpose is to separate one diff from another (ADR 0004).
- **ADRs to read first:**
  - 0003 — managed storage layout; a Lesson directory sits beside `branches/` under the repo.
  - 0004 — view buckets exist to separate diffs; a Lesson has none, so it does not belong in one.
  - 0010 — staleness is two conditions today; a Lesson adds a third kind, kept out of `is_stale` the way tours are.
  - 0021 — every spawn path that runs a review agent takes a slot; using `spawn_background_agent_task` keeps that true.
  - 0022 — an agent whose prompt holds untrusted content runs read-only and the host writes; read-only is enforced per CLI family.
  - 0023 — prompts are compiled in; a learning preference is user data appended at runtime, never a prompt file.
  - 0025 — tours are bound to a diff hash; contrast for why a Lesson is not a Tour.
  - 0041 — document anchors (`hunk_index: None`, `line_start: Some`) are the precedent for a line reference outside any hunk.

## Scope boundaries
- **In scope:** ADR + `CONTEXT.md` terms; a Lesson data type, its store, loader and staleness check in `er-engine`; a Lesson prompt builder; host-written, read-only spawn; optional `[learning]` config section; TUI AI Hub entry that starts a Lesson; desktop command and a card listing and showing Lessons.
- **OUT of scope / DO NOT TOUCH:**
  - `preview_file_status` and the Preview refusal. Opening a Lesson's file reference in Preview is a follow-up; in this task the reference is shown as `path:line` text that can be copied or opened in the editor (`open_in_editor` already exists in the desktop command list at `crates/er-desktop/src/main.rs:1916`).
  - Professor, diagram and tour behaviour on diffs, their sidecars and prompts.
  - `spawn_background_importance`'s own write access (see Gotchas). Report it; do not fix it here.
  - er-mcp. No MCP tool for Lessons in this task.
  - Any review behaviour. A Lesson has no findings, severity, or confidence gate.

## Steps
### Step 1 — Write the ADR and the CONTEXT.md terms, then stop
- **Goal:** a person approves the design before any code exists.
- **Why / invariant:** this adds an artifact kind with a new identity and staleness rule; ADR rule in `../README.md` "Project-wide rules" applies. `CONTEXT.md` lists "explainer" under *Avoid* for **Tour**, so the blog's word cannot be the name.
- **Files:** new `docs/adr/<next>-lessons-are-keyed-by-source-objects.md` (find the next number with `ls docs/adr | tail -1`; at c96dc0f12 it was 0043, so expect 0044 unless other tasks took it); `CONTEXT.md`.
- **Do this:**
  1. Add to `CONTEXT.md` under `## Language`, after **Pillar**:
     - **Lesson**: a teaching artifact about a subject in the repo as it is now, written on request. It belongs to the repo and lives outside every view bucket. It goes stale when the code it read changes. _Avoid_: explainer, guide, tour, walkthrough.
     - **Subject**: what a Lesson is about: a set of repo paths, a free-text question, or both. _Avoid_: scope, target, slice. (Check: `scope` is already an *Avoid* word for **View bucket** and for **Reach**; `target` collides with `BackgroundTaskTarget`.)
  2. Write the ADR (format: `.agents/skills/domain-modeling/ADR-FORMAT.md`, short). It must state these decisions, each with its reason:
     - **Storage:** `<storage_root>/repos/<slug_repo(repo_root)>/lessons/<lesson-id>/lesson.json`, outside every view bucket. Reason: ADR 0004 buckets separate diffs; a Lesson has no diff, and a person switching from branch view to Staged should still see it. Local clones only (same refusal as importance), so the repo slug always comes from `slug_repo`.
     - **Identity and staleness:** a Lesson records the commit it was written at and, for every path it read, the git object id at that commit (`git rev-parse <commit>:<path>`; a directory gives a tree id). It is stale when any recorded path's object id at the current `HEAD` differs or the path is gone. Reason: keying on `HEAD` alone would mark every Lesson stale on every unrelated commit; keying on a diff hash is impossible.
     - **Uncommitted changes:** staleness compares committed objects only. If any recorded path had uncommitted changes when the Lesson was written, the Lesson stores `dirty_at_write: true` and the UI says so. Reason: hashing working-tree directories on every check is a cost with no clear payoff.
     - **Write path:** the agent runs read-only and prints JSON between markers; the host validates and writes (ADR 0022). Reason: the prompt carries the user's free-text question and repo content.
     - **Regeneration:** a stale Lesson is flagged and offers Regenerate; nothing reruns on its own (README "Decisions already made": one click, nothing runs on refresh).
     - **Kept out of `is_stale`:** like tours (ADR 0010 Consequences), Lesson staleness is its own flag.
  3. Run `just docs-check`.
  4. Append under `## Comments` in this file: the ADR path, the two new terms, and "awaiting owner approval". Set `Status: needs-design` (unchanged) and stop.
- **Watch out for:** `just docs-check` rejects file tables and inventories in instruction docs; keep the ADR to reasons.
- **Verify:** `just docs-check` → exits 0. `grep -n "^\*\*Lesson\*\*" CONTEXT.md` → one match.
- **If it fails:** fix the doc; do not touch code.

**STOP here. Do not start Step 2 until the owner approves the ADR in writing.**

### Step 2 — Add the Lesson data type, store paths and atomic write
- **Goal:** `crates/er-engine/src/ai/lesson.rs` with the sidecar type, path helpers, parse-validate-write from agent stdout, and a loader.
- **Why / invariant:** the host is the only writer (ADR 0022); fields the harness knows (`id`, `subject`, `commit`, `sources`) are pinned by the host, never taken from the agent's JSON.
- **Files:** new `crates/er-engine/src/ai/lesson.rs`; register in `crates/er-engine/src/ai/mod.rs` next to `pub mod diagrams;` (line 3) and `pub use diagrams::*;` (line 20). `ai` is always compiled (`crates/er-engine/src/lib.rs:8`), so this module must not import `crate::app` or `crate::arena` (both `#[cfg(feature = "ui")]`).
- **Do this:**
  - Type (names may follow the ADR; keep the fields):
    ```rust
    pub const LESSON_JSON_BEGIN: &str = "---LESSON_JSON---";
    pub const LESSON_JSON_END: &str = "---END_LESSON_JSON---";

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Lesson {
        pub version: u32,
        #[serde(default)] pub id: String,          // from the directory name, pinned by host
        #[serde(default)] pub created_at: String,
        #[serde(default)] pub question: String,    // pinned by host
        #[serde(default)] pub paths: Vec<String>,  // subject paths, pinned by host
        #[serde(default)] pub commit: String,      // HEAD at write, pinned by host
        #[serde(default)] pub sources: BTreeMap<String, String>, // path -> object id, host-computed
        #[serde(default)] pub dirty_at_write: bool,
        #[serde(default)] pub title: String,
        pub summary: String,                       // markdown
        #[serde(default)] pub insights: Vec<LessonInsight>,
        #[serde(default)] pub mermaid: String,     // optional, bare source
        #[serde(skip)] pub stale: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LessonInsight {
        pub title: String,
        #[serde(default)] pub description: String,
        pub path: String,
        #[serde(default)] pub line_start: Option<usize>,
        #[serde(default)] pub line_end: Option<usize>,
    }
    ```
    An insight is not a `Finding`: a Finding is AI review output about a place in a diff (`CONTEXT.md`), and reusing it would pull severity, confidence and the gate into a teaching artifact.
  - `pub fn lessons_dir(repo_root: &str) -> PathBuf` → `storage_root().join("repos").join(slug_repo(repo_root)).join("lessons")`. Respect `ER_REPO_LOCAL` the way `local_branch_bucket_dir` does (`crates/er-engine/src/storage.rs:150`): under repo-local mode use `<repo_root>/.er/lessons`.
  - `pub fn new_lesson_id() -> String` → `lesson-<unix ms>`, the same pattern as `diagram_output_file` for `custom` (`crates/er-engine/src/ai/diagrams.rs:139`). Add an `is_safe_lesson_id` check like `is_safe_diagram_id` (`:117`).
  - `pub fn persist_lesson_from_agent_stdout(stdout: &str, stream_json: bool, pinned: LessonPins, repo_root: &str, output_path: &Path) -> Result<()>`: extract the agent text with `extract_agent_stdout_text` (make it, `strip_optional_fence` (`:292`) and `extract_fenced_json` (`:307`) `pub(crate)` in `diagrams.rs`; visibility is the only change to that file), then the marked payload: `extract_diagram_json_payload` (`diagrams.rs:267`) hard-codes `DIAGRAM_JSON_BEGIN`/`DIAGRAM_JSON_END`, so copy it into `lesson.rs` with the Lesson markers. Parse, then:
    - `LessonPins` is a plain struct the spawn fills: `id`, `question`, `paths`, `commit`, `dirty_at_write`. Overwrite those fields, and `created_at` when empty, from `pinned`;
    - read the agent's `sources` list (paths it says it read; accept a JSON array of strings in the agent payload under `"read"`), union with `paths`, drop anything that fails the path check in Step 2's Watch out, cap at 200, compute each object id (Step 3's helper), store in `sources`;
    - reject an empty `summary`; trim and size-cap `mermaid` at 200 000 bytes like diagrams;
    - drop insights whose `path` is not in `sources`.
    - Atomic write: copy `write_diagram_atomic` (`crates/er-engine/src/ai/diagrams.rs:250`).
  - `pub fn load_lessons(repo_root: &str) -> Vec<Lesson>`: read every `<id>/lesson.json`, newest first, size-capped like `read_professor_sidecar` (`crates/er-engine/src/ai/professor.rs:37`, `MAX_SIDECAR_BYTES`).
- **Watch out for:** paths from the agent are untrusted. Reject absolute paths, any `..` component, and anything that `git rev-parse <commit>:<path>` does not resolve. The existing check is private to `app/` (`crates/er-engine/src/app/state/preview.rs:374`, `fn validate_path`, which rejects any path component that is not `Component::Normal`), and `app` is behind the `ui` feature, so copy its five lines into `lesson.rs` rather than importing it.
- **Verify:** add tests in a `#[cfg(test)] mod tests` in `lesson.rs`, copying the shape of `persist_diagram_from_marked_payload_writes_only_target_file` (`crates/er-engine/src/ai/diagrams.rs:377`):
  - `persist_pins_host_fields_over_agent_values` (agent JSON carries a wrong `question` and `commit`; file on disk has the pinned ones);
  - `persist_rejects_empty_summary` (file not written);
  - `persist_drops_paths_outside_repo` (`../etc/passwd` and `/abs` never reach `sources`). This and the first test need a temp git repo with one commit, because object ids come from git; build it as in Step 3.
  `~/.claude/scripts/locked.sh . cargo test -p er-engine lesson::` → `test result: ok. 3 passed` (N ≥ 3; a filter that matches nothing also prints `ok`, so check the count).
- **If it fails:** fix the module; do not touch `diagrams.rs` beyond the visibility change.

### Step 3 — Object-id helper and the staleness check
- **Goal:** compute object ids at a commit, and mark a loaded Lesson stale.
- **Why / invariant:** stale means "the code this Lesson read has changed since"; an unrelated commit must not mark it stale.
- **Files:** `crates/er-engine/src/ai/lesson.rs`.
- **Do this:**
  - `pub fn head_commit(repo_root: &str) -> Result<String>` (`git rev-parse HEAD`) and `pub fn object_id_at(repo_root: &str, commit: &str, path: &str) -> Option<String>` (`git rev-parse <commit>:<path>`, `None` on failure). Shell out with `std::process::Command::new("git")` per ADR 0001. There is no shared HEAD-sha helper in `crates/er-engine/src/git/` at c96dc0f12; `TODO(executor): confirm none was added since — search for "\"rev-parse\", \"HEAD\"" outside tests`.
  - `pub fn mark_stale(lessons: &mut [Lesson], repo_root: &str)`: one `git rev-parse HEAD`, then for each Lesson compare every `sources` entry with `object_id_at(HEAD, path)`; any mismatch or `None` sets `stale = true`. Batch with a single `git rev-parse HEAD:<p1> HEAD:<p2> …` per Lesson if the count is large.
  - `dirty_at_write`: at write time, `git status --porcelain -- <paths…>` non-empty → `true`.
- **Watch out for:** run these off the UI lock where the caller holds one (the desktop runs commands in `run_blocking`).
- **Verify:** tests with a temp repo, using a local `git` helper like the one in `crates/er-engine/src/app/state/preview.rs:642` (sets `GIT_CONFIG_GLOBAL=/dev/null`):
  - `lesson_is_fresh_after_an_unrelated_commit` (commit touching another file → `stale == false`);
  - `lesson_is_stale_when_a_source_file_changes`;
  - `lesson_is_stale_when_a_source_file_is_deleted`.
  `~/.claude/scripts/locked.sh . cargo test -p er-engine lesson::` → all pass, count includes the 3 new tests. Then plant a break (make `mark_stale` always return early), rerun, see `lesson_is_stale_when_a_source_file_changes` fail, revert in the same command.
- **If it fails:** stop and ask if git output differs by platform.

### Step 4 — Read-only spawn and host write
- **Goal:** `App::spawn_background_lesson(&mut self, question: Option<String>, paths: Vec<String>) -> Result<()>` that queues a read-only agent and, on success, persists the Lesson from stdout.
- **Why / invariant:** the agent must have no write path on any CLI family (ADR 0022), and the spawn must take an agent slot (ADR 0021).
- **Files:** `crates/er-engine/src/app/state/comments.rs` (beside `spawn_background_importance`, `:2869`), `crates/er-engine/src/app/state/background.rs`.
- **Do this:**
  1. Carry the write target through the queue. A spawn does not launch directly: `spawn_background_agent_task` builds a `PendingBackgroundTask` (about `comments.rs:3075`), which may wait in `pending_background_tasks`, and `fn launch_background_agent_task` (`comments.rs:3115`) destructures it later. So:
     - add `HostWriteLesson { output_path, pins, repo_root }` in `background.rs` beside `HostWriteDiagram` (`:146`);
     - add `pub host_write_lesson: Option<HostWriteLesson>` to `pub struct PendingBackgroundTask` (`background.rs:130`, next to `host_write_diagram` at `:137`);
     - add an `Option<HostWriteLesson>` parameter to `spawn_background_agent_task`, store it in the pending task, and destructure it in `launch_background_agent_task`;
     - update the seven existing callers (`comments.rs:2814`, `:2835`, `:2852`, `:2909`, `:2990`, `:3007`, `:3024`) to pass `None`.
  2. Make every read-only decision cover a Lesson. Two places key on the diagram target today, and both must change:
     - `comments.rs:3240` `let readonly_run = host_write_diagram.is_some();` (Codex sandbox, Cursor plan mode, OpenCode permission env, `--add-dir` withholding);
     - `comments.rs:3333` `let allowed: &[&str] = if host_write_diagram.is_some() {` (Claude's `--allowedTools` list; the other branches grant `Write` and `Edit`). Claude is the default provider, so missing this one leaves the common case writable.
     Compute one `let host_writes = host_write_diagram.is_some() || host_write_lesson.is_some();` before the thread and use it at both places. To make the Claude choice testable, lift the three lists at `:3333` into a module-level `fn claude_allowed_tools(read_only: bool, prepared_diff: bool) -> &'static [&'static str]` that returns exactly the current lists, and call it there.
  3. In the worker's completion code, beside the diagram branch (`:3539`), add: if `host_write_lesson` is set, call `crate::ai::persist_lesson_from_agent_stdout(&stdout_lines.join("\n"), is_stream_json, …)` with `.with_context(...)` like the diagram branch.
  4. `spawn_background_lesson`: refuse `self.tab().is_remote()` with "A lesson needs a local clone of this repo"; refuse when both `question` and `paths` are empty; compute `head_commit`; create the Lesson dir; build the prompt (Step 5); build a `BackgroundTaskTarget` with `er_dir` = the Lesson directory, `scope: format!("lesson:{id}")`, `managed_local: true`.
- **Watch out for:**
  - `managed_local: true` is required. The worker picks the agent's working directory at `:3396`: `managed_local || (!is_remote && !is_cache_er_dir)` → `repo_root`, else `er_dir`. The Lesson dir is outside the repo, so without `managed_local` the agent would start in the empty Lesson dir and read nothing.
  - Kind string: `lesson`. Dedup at `:3060` is by `kind` + `target`; `BackgroundTaskTarget::key()` (`background.rs:34`) ignores `er_dir`, so two Lessons for one repo with the same `scope` value would collide. Put the Lesson id into `scope` (`format!("lesson:{id}")`) so two different Lessons can queue.
  - Do not copy `spawn_background_importance`'s `None` for the write target: that agent runs without read-only enforcement today (see Gotchas).
  - The other families' read-only arguments already have config-level tests (`apply_readonly_spawn_narrows_the_codex_sandbox`, `crates/er-engine/src/config/mod.rs:3868`; `apply_opencode_readonly_spawn_sets_auto_and_readonly_env`, `:3856`). There is no test of the Claude list at c96dc0f12; the lift in item 2 is what makes one possible.
  - A second invocation path exists, `crate::agent_runtime` with `AgentAccessProfile::ReadOnly` (used by `crates/er-engine/src/app/card_ai_spawn.rs:55`). Do not migrate to it in this task.
- **Verify:**
  - Record the diagram test count first: `~/.claude/scripts/locked.sh . cargo test -p er-engine diagram` → note N.
  - Add tests in `comments.rs`'s `mod background_queue_tests` (`crates/er-engine/src/app/state/comments.rs:3974`): `read_only_claude_tools_grant_no_write_or_edit` (no `"Write"`, no `"Edit"`, and no `Bash(mkdir*)`/`Bash(cp .er/*)` when `read_only` is true) and `writable_claude_tools_are_unchanged` (both `prepared_diff` cases return exactly today's lists). `~/.claude/scripts/locked.sh . cargo test -p er-engine claude_tools` → 2 passed. Plant a break (return the `prepared_diff` list for `read_only`), see the first test fail, revert in the same command.
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine diagram` → N passed, unchanged.
- **If it fails:** stop; do not change diagram read-only behaviour to make a lesson test pass.

### Step 5 — Lesson prompt builder
- **Goal:** `pub fn build_lesson_prompt(repo_root: &str, question: Option<&str>, paths: &[String], learning_style: Option<&str>) -> String` in `crates/er-engine/src/ai/prompts.rs`, next to `build_importance_prompt` (`:961`).
- **Why / invariant:** prompts are compiled in (ADR 0023); the free-text question and learning style are user data placed in clearly labelled sections, as `build_diagram_prompt_prepared_diff` does with "Custom instructions (user-provided)".
- **Do this:** build with `push_str` (like `:681`), never one `format!`, so braces in the question cannot break formatting. Sections:
  1. Role: teach a skilled developer how this part of the repo works today. Teaching tone: purpose, structure, data flow, invariants, one or two non-obvious mechanisms. Not a review: do not flag bugs or style.
  2. Work in `<sanitize_for_shell(repo_root)>`; read the subject paths (listed) and follow references as needed; stay inside the repo (copy the first bullet of the "### Scope" block in `review_rules_preamble_with_hash`, `prompts.rs:160`; leave out the PR-fetch and no-checkout bullets).
  3. Output: emit JSON between `LESSON_JSON_BEGIN` / `LESSON_JSON_END` in the final reply; no Write, Edit, or redirects. Fields: `title`, `summary` (3–6 short markdown paragraphs), `insights` (≤ 12, each with `path`, `line_start`, `line_end`, `title`, `description`), `read` (paths read), optional `mermaid`.
  4. Mermaid rules: copy the "Mermaid rules" block from `build_diagram_prompt_prepared_diff` (`prompts.rs:732`; the MCP builder repeats it at `:807`). Better: lift it into a `const MERMAID_RULES: &str` used by both builders, if that is the only change to the diagram builder and its tests still pass.
  5. `## Question (user-provided)` when set; `## Subject paths` when set; `## How this person likes to learn (user-provided)` when `learning_style` is set.
- **Watch out for:** do not call `professor_rules_preamble` or `review_rules_preamble_with_hash`. Both hard-code diff rules ("Findings **only** on `+` or `-` lines", "Set `outside_diff: false`", the `diff-tmp` hash steps), and `PROFESSOR_SUMMARY_FOCUS` (`crates/er-engine/src/ai/professor.rs:94`) says "what this diff implements".
- **Verify:** tests in the `prompts.rs` test module (`crates/er-engine/src/ai/prompts.rs:1923`, `mod tests`):
  - `lesson_prompt_never_mentions_a_diff` (no `diff-tmp`, no `diff_hash`, no `+` or `-` line rule);
  - `lesson_prompt_includes_question_and_style_only_when_set`;
  - `lesson_prompt_forbids_writes` (contains "Do **not** use Write").
  `~/.claude/scripts/locked.sh . cargo test -p er-engine lesson_prompt` → 3 passed.
- **If it fails:** fix the builder.

### Step 6 — Optional `[learning]` section in global config
- **Goal:** `[learning] style = "<free text>"` in global config, read when building the prompt.
- **Why / invariant:** config is global-only (ADR 0005); no repo-local file. A missing section must change nothing.
- **Files:** `crates/er-engine/src/config/mod.rs` — add `#[serde(default)] pub learning: LearningConfig,` to `ErConfig` (`:24`), and `#[derive(Debug, Clone, Default, Serialize, Deserialize)] pub struct LearningConfig { #[serde(default)] pub style: Option<String> }` near `SummaryConfig` (`:541`).
- **Do this:** pass `self.config.learning.style.as_deref()` from `spawn_background_lesson`. A blank string counts as unset.
- **Watch out for:** desktop Settings edits config through string keys in `apply_config_field` (`crates/er-engine/src/config/desktop_settings.rs:180`, e.g. `"summary.push_to_pr"`). A new section needs no change there unless Settings should edit it; a Settings field is out of scope, so users set `[learning]` by hand in `config.toml`.
- **Verify:** tests in the config test module (`crates/er-engine/src/config/mod.rs:2344`): `learning_section_is_optional` (TOML without it parses, `style` is `None`) and `learning_style_round_trips`. `~/.claude/scripts/locked.sh . cargo test -p er-engine learning_` → 2 passed.
- **If it fails:** stop if config loading rejects unknown sections in a way that breaks older configs.

### Step 7 — TUI entry
- **Goal:** AI Hub item "Teach me…" that asks for a question or paths and starts the spawn. On completion the TUI shows a notification naming the Lesson. Reading Lessons in the TUI is a follow-up (see When to STOP): the TUI has no diagrams view today to copy (`grep -rln diagram crates/er-tui/src/ui` finds nothing at c96dc0f12).
- **Files:** `crates/er-engine/src/app/state/mod.rs:526` (`enum HubAction`, add `RunLesson`), `:6808` (add a `HubItem` after "Propose importance rules"); `crates/er-engine/src/app/state/mod.rs:327` (`enum InputMode`, add `LessonPrompt`); `crates/er-tui/src/input/mod.rs:282` (hub dispatch: set `app.input_mode = InputMode::LessonPrompt`), `crates/er-tui/src/main.rs:482` (key routing, beside `InputMode::BaseBranch => handle_base_branch_input(app, key)?`), `crates/er-tui/src/ui/status_bar.rs:803` and `:1000` (the one-line prompt rendering for `BaseBranch`).
- **Do this:** model the new mode on `InputMode::BaseBranch` end to end: entered from `crates/er-tui/src/input/mod.rs:506`, handled by `pub fn handle_base_branch_input(app: &mut App, key: KeyEvent) -> Result<()>` (`crates/er-tui/src/input/mod.rs:699`), drawn in the status bar. On Enter: input starting with `paths:` is a comma-separated path list; anything else is the question. Call `app.spawn_background_lesson(...)` and notify on error, as the `RunImportanceAgent` arm does.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-tui` → passes. Manual: `just run` in this repo, open the AI Hub, choose "Teach me…", type "How does er load config?", Enter, wait for the task to finish, see the notification and the file under `<storage_root>/repos/<repo>/lessons/`.
- **If it fails:** stop and ask; do not reshape the hub.

### Step 8 — Desktop command and card
- **Goal:** `generate_lesson(question: Option<String>, paths: Vec<String>)` and `delete_lesson(id: String)` Tauri commands, the Lessons list in the snapshot, and a card.
- **Files:** `crates/er-desktop/src/commands.rs` (copy `run_importance_agent`, `:4018`, which wraps the engine call in `run_blocking`); register in `crates/er-desktop/src/main.rs` next to `commands::generate_diagram` (`:1914`); snapshot field in `crates/er-desktop/src/snapshot.rs` (ADR 0006: add an explicit field, do not widen an existing one); UI: a `LessonsCard.svelte` modelled on `desktop-ui/src/lib/components/DiagramsCard.svelte` (task correlation by `t.kind`, a text box like its custom prompt, `MermaidDiagram` for the optional diagram). Types in `desktop-ui/src/lib/types.ts`.
- **Watch out for:** mount the card beside `<DiagramsCard {ai} />` in `desktop-ui/src/lib/components/RightPanel.svelte:416`. Lessons are repo-level, so feed the card from the new snapshot field, not from `ai` (which is per view bucket). Add a Storybook story beside the diagrams one if one exists (`desktop-ui/src/lib/stories/fixtures.ts`).
- **Verify:** `~/.claude/scripts/locked.sh . just lint-ui` → exit 0; `~/.claude/scripts/locked.sh . just test-ui` → pass. Manual: generate a Lesson, commit an unrelated file, Lesson stays fresh; edit a source file and commit, Lesson shows stale with Regenerate.
- **If it fails:** stop after three attempts and report.

## Gotchas & landmines
- **The importance agent is not read-only today.** `spawn_background_importance` (`comments.rs:2869`) passes `None` for the diagram write target with `prepared_diff: false`, so `readonly_run` (`:3240`) is false and Claude gets the last list at `:3333`, which includes `Write` and `Edit`; its doc comment's "the agent has no write path" holds only by prompt wording. Do not copy that spawn's write-target argument. Mention this in your final report; fixing it is outside this task.
- **Two repo slugs exist.** Branch buckets use `slug_repo` (origin basename, `storage.rs:63`); PR buckets use `owner_repo_storage_slug` (`owner-repo`, `crates/er-engine/src/github.rs:3243`). Lessons use `slug_repo` and require a local clone.
- **The worker's working directory** depends on `managed_local` (`comments.rs:3396`). Set it.
- **Dedup key ignores `er_dir`.** See Step 4.
- **`ai` is always compiled; `app` and `arena` are behind `ui`.** `lesson.rs` must not import either, or the headless build (`just build-engine-headless`) breaks.
- **A reload replaces `AiState` wholesale** (`crates/er-engine/src/ai/CLAUDE.md`, Traps). Keep Lessons out of `AiState`; they are repo-level and loaded separately.

## Decision log (forks pre-answered)
- **If you are tempted to store a Lesson in the active view bucket:** don't. Buckets separate diffs (ADR 0004) and a Lesson would vanish on a mode switch.
- **If you want to reuse `ProfessorReview` / `Finding` for insights:** don't. A Finding carries severity, confidence and the gate, and `CONTEXT.md` defines it as review output about the diff.
- **If you want staleness keyed on `HEAD`:** don't; it goes stale on every commit. Per-path object ids.
- **If the agent reports a path that does not resolve at the commit:** drop it from `sources` and drop insights pointing at it; do not fail the whole Lesson.
- **If the user gives only a question:** `paths` is empty and `sources` comes only from the agent's `read` list. If that list is empty after filtering, still write the Lesson with empty `sources` and show "no sources recorded" in the UI; such a Lesson can never be marked stale, and the UI must say so. Record this in the ADR.
- **If a diagram is wanted:** it rides inside the Lesson (`mermaid`), not as a `diagrams/*.json` file, because diagram sidecars live in a view bucket and are keyed by diff hash.
- **Name:** "Lesson". If the owner rejects it in Step 1, use the name they choose everywhere, including kind strings.
- **Task 18 builds on these names (consistency pass, 2026-10-17):** `head_commit`, `object_id_at`, `mark_stale` and the copied path check (Steps 2–3), `persist_lesson_from_agent_stdout` with `LessonPins` (Step 2), `HostWriteLesson` and `PendingBackgroundTask.host_write_lesson`, `claude_allowed_tools` and `spawn_background_lesson` (Step 4), and `InputMode::LessonPrompt` (Step 7). 18 Step 2 moves the commit, object-id, path-check and stale-comparison helpers into a shared `ai/subject.rs`, and 18 Step 5 turns `Option<HostWriteLesson>` into an enum. If you rename any of these, record the new names under `## Comments` so 18 can find them.

## Full verification
1. `~/.claude/scripts/locked.sh . cargo test -p er-engine lesson` → all new tests pass (≥ 9 across Steps 2, 3, 5).
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine learning_` → 2 passed.
3. `~/.claude/scripts/locked.sh . cargo test -p er-engine` → full engine suite passes.
4. `~/.claude/scripts/locked.sh . cargo test -p er-tui` → passes.
5. `~/.claude/scripts/locked.sh . just build-engine-headless` → builds (proves `lesson.rs` does not reach into `app`).
6. `~/.claude/scripts/locked.sh . just clippy` → exit 0. `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
7. `just docs-check` → exit 0; `just comments-check` → exit 0.

## Rollback
Revert the commits. Lesson files live only under `<storage_root>/repos/<repo>/lessons/`; deleting that directory removes all user data this task created. No existing sidecar format changes.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- After Step 1, always, until the owner approves.
- The owner wants Lessons to open files in Preview: that means changing `preview_file_status` and is its own design question.
- The owner wants to read Lessons inside the TUI: that needs a new overlay, which this task does not design.
- The owner wants Lessons shared with the team (committed to the repo or uploaded): that conflicts with managed storage (ADR 0003) and needs its own ADR.
- Making the Lesson agent read-only would change diagram behaviour in any way.

## Comments
