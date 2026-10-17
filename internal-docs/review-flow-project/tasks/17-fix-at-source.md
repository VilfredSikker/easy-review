# 17 — Fix repeated findings at the source

Status: needs-design
Idea: 18 in the original idea list (summarised in ../README.md)
Depends on: 03-finding-outcomes.md, 05-review-md.md (the drafting can run without 05; see Decision log)
Impact: M · Complexity: M · Size: M
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.
>
> **This task is `needs-design`.** Step 1 is a design note that a person must approve. Write it, then STOP and ask. Write no code before the approval is recorded under `## Comments`.

## In plain terms
- **The problem** — The same kind of finding keeps coming back in a project. The reviewer flags something the team does on purpose, and people dismiss it as "wrong" or "intentional" again and again. Or agents keep making the same mistake, and people mark it "fixed" again and again. Each time costs a person's attention or an agent round, and nothing learns from it.
- **Why it matters** — One line in `REVIEW.md` stops a false positive for good. One rule in `CLAUDE.md` or one lint rule stops a repeated bug before review even sees it. er already records how people answered each finding (task 03), so it can spot the repeats and draft that line.
- **The fix** — er groups recorded finding outcomes per project, finds the kinds that repeat, and drafts a suggestion: a `REVIEW.md` line, a `CLAUDE.md` rule, or a lint rule. The person reads the draft and copies it, or declines it. er never edits those files itself, and it remembers declined drafts so it does not suggest them again.
- **TL;DR** — Turn repeated answers to findings into a drafted fix the person can copy into the repo.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> **When the automated reviewer keeps flagging the same thing, we fix it at the source:** in the skill, with a lint rule, or with updates to instructional files

> When a review misses something, we ask what the model didn't have in front of it and put the fix into a `CLAUDE.md`, a skill or a new check.

> Add a `REVIEW.md`, and move repeated review comments into it.

## Mission & definition of done
Design, get approval for, then build a per-project "fix at the source" feature. It reads task 03's finding outcomes across every view bucket of a project, groups them into classes, picks classes that repeat in one direction, and has a read-only agent draft a suggestion for each picked class. The person copies or declines each draft. er writes only its own record of drafts and decisions, never `REVIEW.md`, `CLAUDE.md`, a skill, or a lint config.

- [ ] Step 1's design note is written and approved by the person (recorded under `## Comments` with the date).
- [ ] An engine function lists repeat candidates for a project from outcomes in **both** bucket trees. A test proves a one-tree walk would miss records.
- [ ] Drafting runs a read-only agent through an agent slot. The host parses its reply and stores the draft.
- [ ] No code path writes `REVIEW.md`, `CLAUDE.md`, any skill file, or any lint config. A test or grep proves it.
- [ ] TUI and desktop both list drafts with Copy and Decline. Declined drafts are not suggested again for the same class.
- [ ] ADR and `CONTEXT.md` entries as the approved design requires.

**Risk:** ordinary. The feature only drafts text. Its risk is a drafted rule that is wrong, which the person screens before copying.

## Before you start (orientation)
- **Branch:** see `../README.md` "Branching". Feature work: the current release branch.
- **Prerequisite:** task 03 is done. Check `grep -n "pub fn load_finding_outcomes" -r crates/er-engine/src/ai`. If it finds nothing, STOP.
- **Commands** (one at a time, through the lock):
  - Engine: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine <filter>`
  - TUI: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui <filter>`
  - Headless engine build (the walker must not depend on `app`): `~/.claude/scripts/locked.sh . just build-engine-headless`
  - Desktop UI: `~/.claude/scripts/locked.sh desktop-ui bun run test`; `~/.claude/scripts/locked.sh desktop-ui bun run check`
  - Desktop Rust (slow; last): `~/.claude/scripts/locked.sh . cargo test -p er-desktop <filter>`
- **The map** (confirmed at c96dc0f12 unless marked):
  - Task 03's record (03-finding-outcomes.md Step 2): module `crates/er-engine/src/ai/outcomes.rs`, re-exported from `er_engine::ai`. `pub fn load_finding_outcomes(er_dir: &str) -> FindingOutcomes` (absent or malformed reads as empty); `FindingOutcomes { version, outcomes: Vec<FindingOutcome> }`; `FindingOutcome { key, finding_id, file, line_start, title, category, lens, raised_by, severity, confidence, outside_diff, outcome: OutcomeKind, reason, diff_hash, recorded_at }`; `OutcomeKind { Fixed, Wrong, Intentional, DoesNotMatter }`, serialized as `"fixed" | "wrong" | "intentional" | "does_not_matter"`. One file per view bucket at `<er_dir>/finding-outcomes.json`, append-only (a repeat on the same `key` + `diff_hash` replaces), never filtered by staleness. The record has no `pattern` field and no `pre_existing` copy.
  - `crates/er-engine/src/storage.rs:111` — `pub fn branch_dir(repo_slug, branch_slug)`. Line 136 — `pub fn view_bucket_dir(repo_slug, branch_slug, bucket)` (`repos/<repo_slug>/branches/<branch>/view-buckets/<bucket>`). Line 162 — `pub fn pr_bucket_dir(owner_repo_slug, pr_number)` (`repos/<owner_repo_slug>/prs/pr-<N>`). Line 51 — `pub fn rules_key(repo_root: &str, remote_repo: Option<&str>) -> String`. Line 63 — `pub fn slug_repo(repo_root: &str) -> String` (origin basename). Line 14 — `pub fn storage_root()` (honours `ER_STORAGE_ROOT`).
  - `crates/er-engine/src/github.rs:3243` — `pub fn owner_repo_storage_slug(owner: &str, repo: &str) -> String` (`owner-repo`). Line 2888 — `pub fn canonical_owner_repo_slug(repo_root: &str) -> Option<String>`.
  - `crates/er-engine/src/sidecar_summary.rs:195` — `pub fn list_repo_pr_artifacts(...)`, and `:207` — `pub fn list_repo_pr_artifacts_in_dir(owner, repo, prs_dir: &Path, kinds_filter, limit)`. These are the PR-bucket walker and its test-friendly `_in_dir` split; copy the split.
  - `crates/er-engine/src/ai/identity.rs:10` — `pub fn canonical_finding_text(text: &str) -> String`.
  - Read-only agent pattern: `crates/er-engine/src/ai/prompts.rs:947-948` (`IMPORTANCE_JSON_BEGIN` / `_END` markers), `:961` (`pub fn build_importance_prompt`). `crates/er-engine/src/config/mod.rs:135-151` (`ImportanceProposal`, `from_reply`). `crates/er-engine/src/app/state/comments.rs:2869` (`pub fn spawn_background_importance`). `:3518-3535` (the worker parses the reply). `:3232-3256` (read-only spawn flags: `apply_readonly_spawn`, `apply_opencode_readonly_storage_spawn`, used today for diagrams only).
  - `crates/er-engine/src/app/state/comments.rs:2048` — `pub(super) fn copy_to_clipboard(text: &str) -> Result<()>`.
  - `crates/er-engine/src/app/state/mod.rs:6808-6815` — the "Propose importance rules" hub item, the nearest existing hub row for "agent proposes something about the repo".
  - Task 05's base-branch `REVIEW.md` reader (05-review-md.md Steps 3–4): `er_engine::ai::review_md::collect(source: &ReviewMdSource, raw_diff: &str) -> Option<String>` in `crates/er-engine/src/ai/review_md.rs`, with the source from `TabState::review_md_source()`. For a project-level draft there is no diff: pass `""`, and `collect` reads only the root `REVIEW.md` (`candidate_paths` of no changed paths is the root alone). Check it landed with `grep -n "pub fn collect" crates/er-engine/src/ai/review_md.rs`.
  - Task 08's defect pattern field (08-review-convergence.md Step 4): `Finding.pattern: String` (`#[serde(default)]`) in `crates/er-engine/src/ai/review.rs`. Task 03's record does not copy it (see the first bullet), so it is not available on a dismissed finding unless 03's format changes. That is Step 1, question 2.
  - Task 19's outcome walk (19-evals-and-metrics.md Step 2) is planned as `crate::review_metrics::project_metrics_in_dirs(branches_dir, prs_dir) -> ProjectMetrics`, which returns totals; this task needs the records themselves. In the recommended order this task lands first. Write the walk as its own always-compiled function that returns the `FindingOutcome` records from both trees, and build `source_fix_candidates` on it, so 19 can call the same walk. If 19 has landed first with a record-level walk, reuse it.
- **Conventions to follow.** The `_in_dir` split, from `crates/er-engine/src/sidecar_summary.rs:195-205`:
  ```rust
  pub fn list_repo_pr_artifacts(owner: &str, repo: &str, kinds_filter: Option<&[SidecarKind]>, limit: usize) -> Vec<ListedPrArtifacts> {
      let slug = owner_repo_storage_slug(owner, repo);
      let prs_dir = storage_root().join("repos").join(&slug).join("prs");
      list_repo_pr_artifacts_in_dir(owner, repo, &prs_dir, kinds_filter, limit)
  }
  ```
  Reply parsing between markers, from `crates/er-engine/src/config/mod.rs:147-151`:
  ```rust
  pub fn from_reply(reply: &str) -> Option<Self> {
      let (_, rest) = reply.split_once(crate::ai::prompts::IMPORTANCE_JSON_BEGIN)?;
      let (body, _) = rest.split_once(crate::ai::prompts::IMPORTANCE_JSON_END)?;
      serde_json::from_str(body.trim()).ok()
  }
  ```
- **Mental model.** Every review artifact lives in managed storage under one of two trees, and the two use **different repo slugs**. Local branch buckets are `repos/<slug_repo>/branches/<branch>/view-buckets/<bucket>`, where the slug is the origin basename, e.g. `easy-review`. PR buckets are `repos/<owner-repo>/prs/pr-<N>`, e.g. `vilfredsikker-easy-review`. Task 03 writes one `finding-outcomes.json` per bucket, and a record keeps a copy of the finding's file, title, category, lens and severity because the finding itself is deleted. A per-project view has to walk both trees and merge them. The importance agent is the precedent for "an agent reads the repo and proposes something". Its reply is parsed by the host, but the host then **writes** global config itself. This task copies the first half only.
- **ADRs to read first:**
  - 0003, 0004 — the two storage trees and per-view buckets. A walk of one tree undercounts without any error.
  - 0005 — config is global-only. Do not add a repo-local file for thresholds or decisions.
  - 0009 — findings are AI-owned. Group on task 03's copies; never reopen deleted findings.
  - 0021 — agent slots. The drafting agent goes through the queue.
  - 0022 — host-written sidecars from an agent's stdout; the read-only spawn per CLI family.
  - 0023 — prompts are compiled in.
  - 0036 — importance as declared config, the closest "agent proposes repo knowledge" decision.

## Scope boundaries
- **In scope:** the design note; a project-wide outcome walk; class grouping and repeat detection; a read-only drafting agent; a host-written store of drafts and decisions; list, Copy and Decline in the TUI and desktop; ADR and `CONTEXT.md` per the approved design.
- **OUT of scope / DO NOT TOUCH:**
  - Writing or patching `REVIEW.md`, `CLAUDE.md`, `AGENTS.md`, any skill, or any linter config. er never edits them.
  - Handing a draft to an agent to apply. The person copies it. A later task may add that, with its own approval.
  - Checking whether the reviewer follows `REVIEW.md` (the blog's "We check separately whether the reviewer actually follows it", quoted in 05). That belongs to task 19's evals.
  - Changing task 03's record format. If the design needs a field task 03 lacks, STOP and ask.
  - Metrics and dashboards (task 19). Running anything on PR refresh or on a timer: drafting starts from a click, per `../README.md`.
  - er-mcp, unless the approved design says otherwise.

## Steps

### Step 1 — Design note (STOP for approval)
- **Goal:** A written, approved answer to each design question in this step, before any code.
- **Why / invariant:** Several answers are product choices, such as where decisions are stored and what counts as "repeated". At least one storage answer is hard to reverse.
- **Files:** append the note under `## Comments` in this file, dated, titled "Design note". If the approved answers include a new storage file or a change to an existing decision, also draft `docs/adr/NNNN-fix-at-source-drafts.md` (next free number when you start; 0044 was free at c96dc0f12, and tasks 05–08 also take numbers).
- **Do this:** answer each question. Pick an option or propose a better one, give one line of reasoning, and mark the recommended option. The options listed here are a starting point.
  1. **Where the outcome data comes from.**
     - (a) **Recommended.** Walk both trees on demand: `branches/*/view-buckets/*` under `slug_repo`, and `prs/pr-*` under `owner_repo_storage_slug`. Reuse task 19's walker if it exists. This matches task 19's rule that metrics are computed on demand, because a second copy drifts.
     - (b) Change task 03 to also append to one project-level log at dismiss time. Faster to read, but it changes a finished task's storage and duplicates data.
     - Say how the two slugs are found from a tab: `slug_repo(repo_root)` for the branch tree, `canonical_owner_repo_slug(repo_root)` or the tab's `remote_repo` for the PR tree. Say what happens for a remote-only tab with no clone (PR tree only).
  2. **What a "finding class" is.**
     - (a) Exact match on `category` + `lens` + `canonical_finding_text(title)`. Cheap and deterministic, but paraphrased titles split one class into many.
     - (b) **Recommended.** Task 08's defect pattern when the record has it, falling back to (a). Note that task 03's record does not copy a pattern field. Decide whether to STOP and ask for one to be added, or to use (a) only.
     - (c) Let the drafting agent cluster titles. Flexible, but not reproducible, and it costs an agent run just to list candidates.
  3. **When a class counts as "repeated", and which way it points.** Recommend: at least 3 records in the class, from at least 2 different `diff_hash` values (so one review cannot qualify alone), with one direction holding a clear majority (at least two thirds):
     - `wrong` or `does_not_matter` → a `REVIEW.md` "do not flag" line (stop a false positive).
     - `intentional` → a `REVIEW.md` "this is deliberate here" line, naming the paths.
     - `fixed` → a `CLAUDE.md` rule or a lint rule (stop the bug before review).
     - Mixed outcomes → no draft.
     - Thresholds are constants in code. ADR 0005 keeps config global, and a per-repo threshold has no home there. Say whether the person agrees.
  4. **Who writes the draft text.**
     - (a) **Recommended.** A read-only agent per selected class, started by a click. It reads the repo (and the base-branch `REVIEW.md` from task 05, if present) and prints one JSON block between new markers, e.g. `===SOURCE_FIX_JSON_BEGIN===` / `===SOURCE_FIX_JSON_END===`, with fields `{ "target": "review_md" | "claude_md" | "lint", "text": "...", "lint_tool": "...", "why": "..." }`. Spawn it read-only with `apply_readonly_spawn` (comments.rs:3248-3249), as diagrams do. The importance agent is only prompt-restricted, which is not enough here.
     - (b) Deterministic templates with no agent: "Do not flag `<title>` in `<paths>`". Free and instant, but too vague to paste as a rule.
     - Say which agent slot workload it uses (ADR 0021).
  5. **Where drafts and decisions are stored.**
     - (a) **Recommended.** A host-written `source-fixes.json` in a project directory `<storage_root>/repos/<slug_repo>/`, through a new `storage.rs` function. Never build the path by hand (ADR 0003). Each entry: class key, direction, counts, draft, status (`proposed` / `copied` / `declined`), timestamps.
     - (b) Per view bucket. Simple, but a decline in one bucket would not stop the same draft appearing in another.
     - (c) In global config. ADR 0005 keeps config for settings, and drafts are data. Not recommended.
     - Name the slug used for remote-only tabs.
  6. **How the person accepts.** Recommend Copy (to the clipboard, `copy_to_clipboard` in the TUI, the existing clipboard path in the desktop) and Decline. Status changes to `copied` or `declined`. A `declined` class is not drafted again unless its record count doubles. Confirm er never writes the target files.
  7. **Where it shows.**
     - TUI: recommend an AI Hub row "Suggest fixes at the source" next to "Propose importance rules" (`mod.rs:6808`). It opens a list overlay of candidates and drafts; Enter drafts or copies, `x` declines.
     - Desktop: a modal through `ui/ModalShell.svelte` (required by `desktop-ui/src/lib/components/agent.md`), opened from the command palette and the AI Review card.
  8. **Without task 05.** Recommend: draft anyway, with no `REVIEW.md` text in the prompt. The draft view says "REVIEW.md not read; check for an existing line before copying".
  9. **Lint rules.** Recommend: the agent names the linter it found (`clippy`, `eslint`, …) and prints a config snippet. If it finds no linter, it drafts a `CLAUDE.md` rule instead. er does not run the linter.
  10. **New words.** Propose `CONTEXT.md` terms, e.g. **Source fix** (a drafted change to a repo's instructions or checks, made from repeated finding outcomes; the person applies it) with an *Avoid* line ("auto-fix", "suggestion" alone).
- **Verify:** the note answers all ten questions, each with one recommended option. `~/.claude/scripts/locked.sh . just docs-check` → exits 0 if an ADR draft was added.
- **If it fails:** n/a. **STOP here and ask the person to approve or amend the note.** Set `Status: needs-design` until they do. Then set it to `in-progress`.

### Step 2 — Project-wide outcome walk and repeat candidates (engine, tests first)
*(Build only after approval. Adjust names to the approved note.)*
- **Goal:** `pub fn source_fix_candidates(branch_tree: &Path, pr_tree: &Path) -> Vec<SourceFixCandidate>`, a pure, test-friendly core, plus a thin wrapper that resolves both paths from a tab's repo.
- **Why / invariant:** Both trees, always. The core takes paths so tests use a tempdir. It lives in the always-on `ai` module and must build headless (`crates/er-engine/src/CLAUDE.md`, "Feature gating").
- **Files:** new `crates/er-engine/src/ai/source_fixes.rs`; `ai/mod.rs` (`pub mod` / `pub use`, beside the `arbiter` lines 16 and 18). If task 19's walker exists, call it.
- **Tests first** (inline, `tempfile::tempdir()`):
  - `candidates_merge_both_bucket_trees` — 2 matching records in a branch bucket and 1 in a PR bucket, from 2 diff hashes → one candidate with count 3. With the PR tree path pointed at an empty dir → no candidate. This is the one-tree guard.
  - `one_review_alone_is_not_a_repeat` — 3 records, same `diff_hash` → no candidate.
  - `mixed_outcomes_draft_nothing` — `wrong`, `fixed`, `intentional` → no candidate.
  - `fixed_majority_points_at_claude_md_or_lint` and `wrong_majority_points_at_review_md`.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine source_fixes` → fail first (`todo!()`), then pass. `~/.claude/scripts/locked.sh . just build-engine-headless` → builds.

### Step 3 — Draft store, prompt, read-only spawn
*(After approval.)*
- **Goal:** Drafting a candidate runs one read-only agent. The host parses its reply and stores the draft in the approved store.
- **Why / invariant:** The agent can write nothing (ADR 0022's read-only spawn). The host writes only er's own store. Do **not** copy `ImportanceProposal::merge_into_global_config` or anything that writes outside er's store.
- **Files:** `crates/er-engine/src/ai/prompts.rs` (new markers and `build_source_fix_prompt`, next to the importance prompt at `:947-990`); `source_fixes.rs` (`SourceFixDraft::from_reply`, copying `ImportanceProposal::from_reply`, and load/save of the store, copying `arbiter.rs:65-83`); `crates/er-engine/src/storage.rs` (the project-dir function); the spawn in `crates/er-engine/src/app/state/comments.rs`, modelled on `spawn_background_importance` (`:2869`), with `readonly_run` true for this kind (`:3240`).
- **Tests:** `draft_is_read_from_between_markers`; `reply_without_markers_stores_nothing`; `prompt_tells_agent_not_to_write` (asserts the prompt says not to use Write, Edit or a redirect). Add a guard test that greps the new module's source for `REVIEW.md`, `CLAUDE.md` and `std::fs::write`, and fails if any write path names a repo file. Keep it simple: assert that every `std::fs::write` call in `source_fixes.rs` targets the store path function.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine source_fix` → pass.

### Step 4 — TUI surface
*(After approval.)* AI Hub row plus list overlay per the note, with Copy through `copy_to_clipboard` and Decline. Test with `make_test_app(make_test_tab(vec![]))` and `app.open_ai_hub()`, matching `OverlayData::ModalHub { items, .. }`, as in the `mod.rs` tests near line 11470. Verify: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui` → pass.

### Step 5 — Desktop surface
*(After approval.)* A Tauri command pair (list and draft; copy and decline) registered in `crates/er-desktop/src/main.rs` and `crates/er-desktop/permissions/app-commands.toml`. A `ModalShell` modal. Calls go through `app.cmd`. Verify: `~/.claude/scripts/locked.sh desktop-ui bun run test` and `… bun run check` → pass. `~/.claude/scripts/locked.sh . cargo check -p er-desktop` → no errors.

### Step 6 — Docs
*(After approval.)* Finish the ADR and the `CONTEXT.md` terms from the note. Verify: `~/.claude/scripts/locked.sh . just docs-check` and `~/.claude/scripts/locked.sh . just comments-check` → both exit 0.

## Gotchas & landmines
- **Two slugs.** `slug_repo` (origin basename) for branch buckets, `owner_repo_storage_slug` (`owner-repo`) for PR buckets. Missing one halves the data silently. Step 2's first test exists for this.
- **Records outlive findings.** Group on task 03's copied fields only. Never look a record's finding up in `review.json`; it was deleted on dismiss.
- **`finding_key` includes the line,** so it is useless as a class key: the same mistake on another line gets another key. Use the approved class definition.
- **The importance worker writes config.** Its parse step is the pattern to copy. Its merge step is the opposite of this task's rule.
- **Prompt injection.** Finding titles came from AI reading untrusted diffs. The drafting agent must run read-only, and its prompt must say the titles are data.
- **No task numbers in code comments** (`just comments-check`).

## Decision log (forks pre-answered)
- **Can er write the drafted line into the file for the person?** No. Copy or decline only. This is the original idea's own rule ("er never edits those files by itself").
- **Shared walk with task 19 (consistency pass, 2026-10-17):** 17 and 19 both walk every bucket of a project for 03's records. Whichever lands first writes one always-compiled record walk over both trees; the other calls it. Both find the branch tree under `slug_repo` (from a clone) or `rules_key("", Some("owner/repo"))` (no clone; 19 uses this), which give the same origin basename, and the PR tree under `owner_repo_storage_slug`.
- **Task 05 not done:** draft without `REVIEW.md` context and say so on the draft (Step 1, question 8).
- **Task 08 not done:** class by exact canonical title (Step 1, question 2a).
- **Task 19's walker exists:** reuse it; do not write a second walk.
- **Task 03's record lacks a field the approved design needs:** STOP and ask. Do not change task 03's format here.
- **The agent prints no markers:** store nothing, and tell the person the draft failed.

## Full verification
*(After Steps 2–6.)*
1. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine source_fix` → all pass.
2. `~/.claude/scripts/locked.sh . just test` → exits 0.
3. `~/.claude/scripts/locked.sh . just build-engine-headless` → builds.
4. `~/.claude/scripts/locked.sh desktop-ui bun run test` and `… bun run check` → pass.
5. `~/.claude/scripts/locked.sh . cargo test -p er-desktop` → exits 0.
6. `~/.claude/scripts/locked.sh . just clippy`, `… just lint-ui`, `… just docs-check`, `… just comments-check` → all exit 0.
7. `git diff --stat` touches no `REVIEW.md`, `CLAUDE.md` or lint config in the repo under review. Check this after a manual draft run against a test repo.
8. Manual: in a test repo, dismiss the same finding as "wrong" on two different diffs, three times in total. Open "Suggest fixes at the source". One candidate shows, pointing at `REVIEW.md`. Draft it, copy it, and the store shows `copied`. Decline a second candidate and it does not reappear.

## Rollback
Revert the commits. The store file in `<storage_root>/repos/<slug>/` is inert without the code; leave it or delete it. Nothing in any repo was changed by er.

## When to STOP and ask
- After Step 1, always.
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- Task 03 is not done.
- The approved design needs a field task 03's record does not have.
- Anyone asks for er to apply a draft to a repo file.

## Comments
