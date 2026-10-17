# 03 — Record an outcome when a finding is dismissed

Status: ready-for-agent
Idea: 2 in the original idea list (summarised in ../README.md)
Depends on: —
Impact: H · Complexity: L · Size: M
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — When a person dismisses an AI finding in the desktop app, er deletes it and keeps no record of why. Was the finding right and already fixed? Wrong? Describing something done on purpose? Not worth the time? That answer is lost. The TUI has no way to dismiss a finding at all.
- **Why it matters** — That answer is the only signal of how noisy the reviewer is. Later tasks need it. Task 08 must not raise a dismissed finding again. Task 09 counts what is still unanswered. Task 17 spots findings that keep coming back. Task 19 builds noise metrics from it. Without a record, none of them has data.
- **The fix** — Dismissing a finding asks one question: fixed, wrong, intentional, or doesn't matter. The desktop also takes an optional reason. er writes the answer to a small file beside the review. The finding still disappears as it does today. The TUI gets the same dismiss action on a key.
- **TL;DR** — Dismiss stops being a silent delete and becomes a recorded answer that later features can read.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> We sort every comment the reviewer leaves by how the person on the PR responded: they fixed it, said it was wrong, said the behavior was intentional, or decided it didn't matter.

> Every finding has to be answered. You either fix it or resolve the thread with a reason.

## Mission & definition of done
Add an engine-owned, append-only record of how a person answered each finding they dismissed. Write it before the finding is deleted, keep it beside the review in the active view bucket, and expose a loader so tasks 08, 09, 17 and 19 can read it. Both front ends dismiss through one engine method. Dismissal still removes the finding exactly as today.

- [ ] `crates/er-engine/src/ai/outcomes.rs` exists, with `load_finding_outcomes(er_dir)` and `record_finding_outcome(er_dir, outcome)`, re-exported from `er_engine::ai`.
- [ ] Dismissing a finding in the desktop writes one record to `<er_dir>/finding-outcomes.json` and then removes the finding, as today.
- [ ] Dismissing the same finding again on the same diff replaces its record. It never adds a second one.
- [ ] The TUI can dismiss the focused finding with `x`, then `f` / `w` / `i` / `m` to pick the outcome. `Esc` cancels.
- [ ] `cleanup_reviews`, `cleanup_review_artifacts` and a new review run leave `finding-outcomes.json` on disk. A test proves it.
- [ ] `CONTEXT.md` defines **Finding outcome**. An ADR records the storage decision.
- [ ] Every test listed in the steps fails before the change and passes after.

**Risk:** ordinary. A new sidecar plus one changed Tauri command. The data it starts collecting is what later tasks depend on, so the record must hold its own copy of the fields they group on.

## Before you start (orientation)
- **Branch:** see `../README.md` "Branching". This is a feature: use the current release branch.
- **Commands** (run one at a time, through the lock):
  - Engine tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine <filter>`
  - TUI tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui <filter>`
  - Desktop Rust tests (compiles Tauri, slow; run last): `~/.claude/scripts/locked.sh . cargo test -p er-desktop <filter>`
  - Desktop UI tests: `~/.claude/scripts/locked.sh desktop-ui bun run test`
  - Type check UI: `~/.claude/scripts/locked.sh desktop-ui bun run check`
- **The map** (all confirmed at c96dc0f12):
  - `crates/er-engine/src/ai/arbiter.rs:14-83` holds the overlay sidecar pattern to copy (struct, `load_arbiter_review`, `write_arbiter_review`).
  - `crates/er-engine/src/ai/identity.rs:28` — `pub fn finding_key(file: &str, line: Option<usize>, text: &str) -> String`.
  - `crates/er-engine/src/ai/finding_cleanup.rs:80` — `pub fn remove_finding_from_sidecars(er_dir: &str, finding_id: &str) -> std::io::Result<bool>`; `:146` — `pub fn delete_threads_linked_to_finding(er_dir: &str, finding_id: &str) -> std::io::Result<bool>`.
  - `crates/er-engine/src/ai/mod.rs:1-30` holds the module list and the `pub use` re-exports.
  - `crates/er-engine/src/ai/review.rs:270` — `pub struct Finding` (`severity: RiskLevel`, `lens`, `category`, `raised_by`, `title`, `line_start`, `outside_diff`, `confidence`).
  - `crates/er-engine/src/app/state/mod.rs:1-8` declares the module list for `app/state/`. `:340` — `pub enum ConfirmAction`. `:3735` — `pub fn reload_ai_state(&mut self)` on `TabState`. `:9023` — `pub fn cleanup_reviews(er_dir: &str)`. `:9037` — `pub fn cleanup_review_artifacts(er_dir: &str)`. `:7329-7344` — help hub rows for `r` and `x`.
  - `crates/er-desktop/src/commands.rs:9020` — `pub async fn dismiss_finding(finding_id: String, view: Option<crate::snapshot::OptimisticView>, state: State<'_, AppState>) -> Result<AppSnapshot, String>`.
  - `crates/er-desktop/src/commands.rs:5060` — `pub fn load_finding_promotions`; `:5068` — `fn save_finding_promotions`. Both are desktop-only.
  - `desktop-ui/src/lib/components/InlineFinding.svelte:62-64` (`function dismiss()`) and `desktop-ui/src/lib/components/AiReviewCard.svelte:527-534` (dismiss button) are the two UI call sites.
  - `desktop-ui/src/lib/optimisticLocal.ts:549` — `function buildDismissFinding(...)`. It reads only `findingId`, so extra args pass through untouched.
  - `desktop-ui/src/lib/components/ui/AnchoredMenu.svelte` and `desktop-ui/src/lib/anchoredMenu.ts` (`anchoredMenuPosition`) are the dropdown pattern; `BranchCard.svelte:222` and `:301` show their use.
  - `crates/er-tui/src/input/normal.rs:147-161` (delete focused comment on `x`), `:192-195` (bare `x` closes the tab), `:162-174` (`r` replies to a focused finding).
  - `crates/er-tui/src/input/mod.rs:766` — `pub fn handle_confirm_input(app: &mut App, key: KeyEvent) -> Result<()>`.
  - `crates/er-tui/src/ui/status_bar.rs:819-846` (confirm prompts, an exhaustive `match`), `:500-514`, `:607-620` and the third hint block that starts near `:660` (hint rows for focused comments and findings).
  - `crates/er-tui/src/main.rs:663-707` (test helpers `make_app`, `send_key`), `:792` and `:834` (the two existing `x` key tests).
- **Conventions to follow.** Atomic sidecar write, from `crates/er-engine/src/ai/arbiter.rs:75-83`:
  ```rust
  pub fn write_arbiter_review(er_dir: &str, review: &ArbiterReview) -> anyhow::Result<()> {
      let path = std::path::Path::new(er_dir).join("arbiter.json");
      let json = serde_json::to_string_pretty(review)?;
      let tmp = path.with_extension("json.tmp");
      std::fs::write(&tmp, json.as_bytes())?;
      std::fs::rename(&tmp, &path)?;
      Ok(())
  }
  ```
  The finding lookup by id, from `crates/er-desktop/src/commands.rs:9078-9093` (`promote_finding_to_comment_impl`). The path is the `review.files` map key:
  ```rust
  let found = app.tab().ai.review.as_ref().and_then(|review| {
      review.files.iter().find_map(|(path, file)| {
          let f = file.findings.iter().find(|f| f.id == finding_id)?;
  ```
  Engine test harness, from `crates/er-engine/src/app/state/comments.rs:4458-4474`. With `ErRoot::RepoLocal(tmp)`, `er_dir()` is `<tmp>/.er`:
  ```rust
  let mut app = App::new_for_test(vec![]);
  let tab = app.tab_mut();
  tab.er_root = ErRoot::RepoLocal(tmp.path().to_string_lossy().to_string());
  let er_dir = std::path::PathBuf::from(tab.er_dir());
  std::fs::create_dir_all(&er_dir).unwrap();
  ```
- **Mental model.** An agent writes `review.json` (and `experts/*.json`, `professor.json`) into the active view bucket (`TabState::er_dir()`). `load_ai_state` (`crates/er-engine/src/ai/loader.rs:208`) merges them and overlays `arbiter.json` last. The finding ids the UI sees may carry an expert prefix (`sec-1`). `remove_finding_from_sidecars` replays that prefix to find the stored record. Dismissal today deletes the finding from its sidecar, deletes linked question, note and GitHub threads, and drops the desktop-only promotions entry. Nothing keeps a copy, so the record has to be built from the loaded finding before the delete.
- **ADRs to read first:**
  - 0003 — sidecars live in managed storage and resolve through `storage.rs`. Use `tab.er_dir()`; never build a bucket path by hand.
  - 0004 — artifacts are per view bucket. The outcome file is too.
  - 0006 — engine state is the UI contract. The dismiss logic goes in the engine; front ends only call it.
  - 0009 — findings are AI-owned. Copying fields into a separate record does not edit the finding; never write the outcome into `review.json`.
  - 0035 — the arbiter overlay. Same "a file beside the findings" idea, but the staleness rule is the opposite (see Step 1).

## Scope boundaries
- **In scope:** the outcome sidecar and its loader/writer; one engine dismiss method; the desktop `dismiss_finding` command and its two UI call sites; a TUI dismiss key with an outcome choice; `CONTEXT.md` term; one ADR.
- **OUT of scope / DO NOT TOUCH:**
  - Hiding a re-raised finding because it already has an outcome. That is task 08's no-repeats rule. Do not add any outcome overlay to `load_ai_state`.
  - Recording anything on promote (`promote_finding_to_comment_impl`, `commands.rs:9135-9150`). Task 08's ledger records promotion.
  - Recording findings that Validate removes (its prompt removes `RESOLVED_OR_INVALID` findings, `crates/er-engine/src/ai/prompts.rs:1432`). That is a machine judgement; a finding outcome records a person's answer.
  - Counting open findings or any "all answered" display (task 09). Any statistics or UI that lists past outcomes (tasks 17, 19).
  - er-mcp (`crates/er-mcp`). Its `pr_feedback_resolve` refuses findings on purpose (`crates/er-mcp/src/server.rs:1181`).
  - Adding outcomes to `AiState`. Nothing in this task reads them at load time.

## Steps

### Step 1 — Write the ADR and the CONTEXT.md term
- **Goal:** The storage decision and its staleness rule are written down before code.
- **Why / invariant:** The file accumulates data that tasks 17 and 19 read months later, so its shape is hard to reverse. Its staleness rule is the opposite of the arbiter's. A reader who knows ADR 0035 will assume otherwise.
- **Files:** new `docs/adr/NNNN-finding-outcomes-are-a-host-written-record.md`. Take the next free number when you start; 0044 was free at c96dc0f12, but tasks 05–08 also add ADRs. Edit `CONTEXT.md` (add after **Arbiter**, before **Importance**). Read `docs/agents/writing-docs.md` first.
- **Do this:** The ADR states these decisions:
  1. A finding outcome is stored in `<er_dir>/finding-outcomes.json`, in the active view bucket. Only er writes it, and no agent prompt names it.
  2. The record keeps its own copy of the finding fields later tasks group on, because the finding itself is deleted right after. This does not edit the finding (ADR 0009).
  3. The record is history. It is never dropped or hidden for a moved diff hash, unlike arbiter verdicts (ADR 0035). `diff_hash` is stored for information only.
  4. Records are appended. A repeat dismissal of the same claim on the same diff (`key` + `diff_hash`) replaces the earlier record, so a retry cannot double-count.
  5. Clearing a review leaves the file in place.
  6. Dismiss still removes the finding.

  Add to `CONTEXT.md`:
  ```
  **Finding outcome**:
  How a person answered a finding they dismissed: fixed, wrong, intentional, or
  doesn't matter, with an optional reason. Recorded beside the review and kept
  after the finding is gone, so noise can be measured later.
  _Avoid_: verdict (the arbiter's), response (a validation reply), resolution
  ```
- **Watch out for:** `CONTEXT.md` **Checklist** already uses "outcomes" for something else. Always write "finding outcome" in full.
- **Verify:** `~/.claude/scripts/locked.sh . just docs-check` → exits 0.
- **If it fails:** fix the link or reference it names. Do not add an inventory table to make it pass.

### Step 2 — The outcome sidecar module (engine, tests first)
- **Goal:** A pure module that loads, upserts and saves outcome records.
- **Why / invariant:** One writer (er), atomic writes, tolerant reads. A malformed file reads as empty and never breaks the review load.
- **Files:** new `crates/er-engine/src/ai/outcomes.rs`. Edit `crates/er-engine/src/ai/mod.rs`: add `pub mod outcomes;` beside `pub mod arbiter;` (line 16), and `pub use outcomes::*;` beside `pub use arbiter::*;` (line 18).
- **Do this:**
  ```rust
  //! `finding-outcomes.json` — how a person answered each finding they dismissed.
  use super::review::{Confidence, Finding, RiskLevel};
  use serde::{Deserialize, Serialize};

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum OutcomeKind { Fixed, Wrong, Intentional, DoesNotMatter }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct FindingOutcome {
      /// `finding_key(file, line_start, title)`, the same key the arbiter matches on.
      pub key: String,
      pub finding_id: String,
      pub file: String,
      #[serde(default)] pub line_start: Option<usize>,
      pub title: String,
      #[serde(default)] pub category: String,
      #[serde(default)] pub lens: String,
      #[serde(default)] pub raised_by: Vec<String>,
      #[serde(default)] pub severity: RiskLevel,
      #[serde(default)] pub confidence: Confidence,
      #[serde(default)] pub outside_diff: bool,
      pub outcome: OutcomeKind,
      #[serde(default)] pub reason: String,
      /// The review's diff hash when this was recorded. Information only.
      #[serde(default)] pub diff_hash: String,
      #[serde(default)] pub recorded_at: String,
  }

  #[derive(Debug, Clone, Default, Serialize, Deserialize)]
  pub struct FindingOutcomes {
      #[serde(default)] pub version: u32,
      #[serde(default, deserialize_with = "lenient_outcomes")] pub outcomes: Vec<FindingOutcome>,
  }
  ```
  - `impl FindingOutcome { pub fn from_finding(path: &str, f: &Finding, diff_hash: &str, outcome: OutcomeKind, reason: &str) -> Self }`. It sets `key = super::identity::finding_key(path, f.line_start, &f.title)`, copies the fields, trims `reason`, and sets `recorded_at = crate::sync::chrono_now()`.
  - `fn lenient_outcomes`: copy `lenient_verdicts` (`crates/er-engine/src/ai/review.rs:134-145`) with the element type changed. Keep it private in `outcomes.rs`.
  - `pub fn load_finding_outcomes(er_dir: &str) -> FindingOutcomes`. Absent, oversized (copy the `MAX_SIDECAR_BYTES` guard, `arbiter.rs:62-72`) or malformed returns `FindingOutcomes::default()`.
  - `pub fn record_finding_outcome(er_dir: &str, outcome: FindingOutcome) -> anyhow::Result<()>`. Load, then remove any record with the same `key` and `diff_hash`, then push, set `version = 1`, and write atomically (copy `write_arbiter_review`). `std::fs::create_dir_all(er_dir)` first.
- **Tests first** (inline `#[cfg(test)] mod tests` in `outcomes.rs`, `tempfile::tempdir()`; build a `Finding` with `serde_json::from_value(json!({...}))` as `arbiter.rs:291-299` does):
  - `record_then_load_round_trips` — every field survives, and `outcome` serializes as `"does_not_matter"` for `DoesNotMatter`.
  - `same_key_same_diff_replaces` — two records, same finding and diff hash, different outcomes → one record, the second outcome.
  - `same_key_new_diff_appends` — same finding, different diff hash → two records.
  - `missing_or_malformed_file_reads_empty` — no file → empty; `"not json"` → empty.
  - `one_bad_entry_keeps_the_rest` — a file with one valid and one invalid entry → one record.
- **Verify:** first write the tests with `todo!()` bodies for the two functions → `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine outcomes` fails. Then implement → same command, all five pass.
- **If it fails:** a name clash from `pub use outcomes::*;` means another module exports the same name. Rename the new item (e.g. `FindingOutcomeRecord`) and say so in `## Comments`. Do not rename the existing one.

### Step 3 — One engine dismiss method (tests first)
- **Goal:** `App::dismiss_finding_with_outcome` is the only place either front end dismisses a finding.
- **Why / invariant:** ADR 0006. The record is built from the loaded finding *before* `remove_finding_from_sidecars`, because afterwards there is nothing to key. Task 08 will hook its ledger into this same method.
- **Files:** new `crates/er-engine/src/app/state/findings.rs`. Declare it in `crates/er-engine/src/app/state/mod.rs:1-8` as `pub(super) mod findings;`, following `pub(super) mod comments;`. Start the file with `use super::*;`, as `comments.rs:1` does.
- **Do this:**
  ```rust
  impl App {
      /// Record how the person answered a finding, then remove it.
      /// `Ok(false)` when the finding is not in the loaded review; nothing is written then.
      pub fn dismiss_finding_with_outcome(
          &mut self,
          finding_id: &str,
          outcome: ai::OutcomeKind,
          reason: &str,
      ) -> Result<bool> {
          let er_dir = self.tab().er_dir();
          let record = self.tab().ai.review.as_ref().and_then(|review| {
              review.files.iter().find_map(|(path, file)| {
                  let f = file.findings.iter().find(|f| f.id == finding_id)?;
                  Some(ai::FindingOutcome::from_finding(path, f, &review.diff_hash, outcome, reason))
              })
          });
          let Some(record) = record else { return Ok(false) };
          ai::record_finding_outcome(&er_dir, record)?;
          let removed = ai::remove_finding_from_sidecars(&er_dir, finding_id)?;
          let _ = ai::delete_threads_linked_to_finding(&er_dir, finding_id);
          self.tab_mut().reload_ai_state();
          Ok(removed)
      }
  }
  ```
  Keep the order: record first, then remove. A failed remove leaves the record in place, and a retry replaces it (Step 2's upsert) without double-counting.
- **Watch out for:** `ai` here is `crate::ai`, imported by `use super::*;` through `mod.rs:10`. `Result` is `anyhow::Result`. If `remove_finding_from_sidecars` returns `Ok(false)` after the record is written, return `Ok(false)` and let the caller report "not found". Leave the record; it describes a finding the person did see.
- **Tests first** (inline `#[cfg(test)] mod tests` at the bottom of `findings.rs`; harness from `comments.rs:4458-4474`; write `review.json` into `<tmp>/.er/` with one finding `f-1` at `src/a.rs` line 10, then call `tab.reload_ai_state()`):
  - `dismiss_records_outcome_then_removes_finding` — after the call, `load_finding_outcomes` has one record with `finding_id == "f-1"` and `outcome == Wrong`. `review.json` on disk has no `f-1`, and the in-memory review has no `f-1`.
  - `dismiss_unknown_id_writes_nothing` — returns `Ok(false)`, and `finding-outcomes.json` does not exist.
  - `outcomes_survive_review_cleanup` — after a dismiss, call `crate::app::cleanup_reviews(&er_dir)` and `crate::app::cleanup_review_artifacts(&er_dir)` (both are re-exported in the `pub use state::{…}` list of `crates/er-engine/src/app/mod.rs`) → `finding-outcomes.json` still exists and loads one record.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine findings::tests` → fails with a `todo!()` body, then all three pass.
- **If it fails:** if the reload empties the review (branch-guard trap, `crates/er-engine/src/ai/CLAUDE.md`, "Traps"), set `"head_branch": "unknown"` in the fixture. Do not change the loader.

### Step 4 — Desktop command takes the outcome
- **Goal:** `dismiss_finding` records the outcome through the engine method.
- **Why / invariant:** One code path for both front ends. The promotions cleanup stays in the desktop, because its helpers live there. The command stays async with `run_blocking` (the source scan at `commands.rs:12381` and `:12476` checks that).
- **Files:** `crates/er-desktop/src/commands.rs:9019-9050`.
- **Do this:** Change the signature to
  ```rust
  pub async fn dismiss_finding(
      finding_id: String,
      outcome: er_engine::ai::OutcomeKind,
      reason: Option<String>,
      view: Option<crate::snapshot::OptimisticView>,
      state: State<'_, AppState>,
  ) -> Result<AppSnapshot, String>
  ```
  Replace the body between `let er_dir = app.tab().er_dir();` and `app.tab_mut().reload_ai_state();` with:
  ```rust
  let er_dir = app.tab().er_dir();
  let removed = app
      .dismiss_finding_with_outcome(&finding_id, outcome, reason.as_deref().unwrap_or(""))
      .map_err(|e| format!("Failed to dismiss finding: {e}"))?;
  if !removed {
      return Err(format!("Finding not found: {finding_id}"));
  }
  let mut promotions = load_finding_promotions(&er_dir);
  if promotions.remove(&finding_id).is_some() {
      save_finding_promotions(&er_dir, &promotions)
          .map_err(|e| format!("Failed to update finding promotions: {e}"))?;
  }
  ```
  Drop the old direct calls to `remove_finding_from_sidecars`, `delete_threads_linked_to_finding` and `reload_ai_state`; the engine method does them. Keep `abort_wrong_view` and `Ok(snap_from(&app, &state))` as they are.
- **Watch out for:** Tauri maps the camelCase JS keys `findingId`, `outcome`, `reason` to these snake_case params. `OutcomeKind` deserializes from `"fixed" | "wrong" | "intentional" | "does_not_matter"`. No new registration is needed: the command name is unchanged in `main.rs:1998` and `permissions/app-commands.toml:96`.
- **Verify:** `~/.claude/scripts/locked.sh . cargo check -p er-desktop` → no errors.
- **If it fails:** a missing `Deserialize` on `OutcomeKind` is the usual cause. Add it in `outcomes.rs`.

### Step 5 — Desktop UI: pick an outcome
- **Goal:** Both dismiss buttons open a small menu with the four outcomes and an optional reason. Picking one sends `dismiss_finding`.
- **Why / invariant:** Every dismissal carries an outcome. No UI path may call `dismiss_finding` without one.
- **Files:**
  - `desktop-ui/src/lib/types.ts`: add `export type FindingOutcomeKind = "fixed" | "wrong" | "intentional" | "does_not_matter";`.
  - New `desktop-ui/src/lib/components/FindingOutcomeMenu.svelte`. It wraps `ui/AnchoredMenu.svelte`. Props: `pos`, `onPick: (outcome: FindingOutcomeKind, reason: string) => void`, `onClose`. Four `role="menuitem"` buttons, labelled "Fixed it", "Wrong", "Intentional", "Doesn't matter", plus one single-line `<input>` for the optional reason. Style the rows like `BranchCard.svelte:301-352`.
  - `desktop-ui/src/lib/components/InlineFinding.svelte:62-64`: `dismiss()` opens the menu, anchored with `anchoredMenuPosition(button.getBoundingClientRect(), window.innerWidth)`. `onPick` calls `app.cmd("dismiss_finding", { findingId: finding.id, outcome, reason: reason.trim() || null })`.
  - `desktop-ui/src/lib/components/AiReviewCard.svelte:527-534`: same, from the trash button. Change its `title` to `"Dismiss finding and say why"`.
- **Watch out for:** `desktop-ui/src/lib/components/agent.md` rules apply. Use `app.cmd`, never raw `invoke`. Do not build command names dynamically. Close the menu on pick and on backdrop click. The optimistic paint (`optimisticLocal.ts:549`) needs no change, because it reads only `findingId`.
- **Tests:** add `desktop-ui/src/lib/components/FindingOutcomeMenu.test.ts` in the style of `ChecklistCard.test.ts:41` (read the component source and assert on it). Assert that `InlineFinding.svelte` and `AiReviewCard.svelte` contain no `app.cmd("dismiss_finding", { findingId: finding.id })` without `outcome`, and that both import `FindingOutcomeMenu`.
- **Verify:** `~/.claude/scripts/locked.sh desktop-ui bun run test` → all pass, including the new file. `~/.claude/scripts/locked.sh desktop-ui bun run check` → 0 errors. `~/.claude/scripts/locked.sh . just lint-ui` → exits 0.
- **If it fails:** if `optimisticLocal.test.ts:515` breaks, it builds the op from `{ findingId: "f-1" }`. That is still valid; fix your change and leave the test as it is.

### Step 6 — TUI: dismiss the focused finding (tests first)
- **Goal:** With a finding focused (`focused_finding_id`), `x` asks "Dismiss finding as: (f) fixed (w) wrong (i) intentional (m) doesn't matter (Esc) cancel". The letter records the outcome and dismisses.
- **Why / invariant:** Bare `x` currently falls through to `close_tab` (`normal.rs:192`) when nothing is focused. That must keep working, and so must `x` on a focused comment. Confirm mode has no text input, so the TUI records an empty reason.
- **Files:**
  - `crates/er-engine/src/app/state/mod.rs:340`: add `DismissFinding { finding_id: String }` to `ConfirmAction`.
  - `crates/er-tui/src/input/normal.rs`: a new arm placed after the focused-comment `x` arm (`:147-161`) and before `KeyCode::Char('x') => { app.close_tab(); … }` (`:192`):
    ```rust
    KeyCode::Char('x')
        if key.modifiers == KeyModifiers::NONE && app.tab().focused_finding_id.is_some() =>
    {
        if let Some(id) = app.tab().focused_finding_id.clone() {
            app.input_mode = InputMode::Confirm(ConfirmAction::DismissFinding { finding_id: id });
        }
        return Ok(());
    }
    ```
  - `crates/er-tui/src/input/mod.rs:766` (`handle_confirm_input`): add arms for `f`, `w`, `m`, and extend the existing `KeyCode::Char('i')` arm (`:866-873`, today guarded for `PushComments`) with an `else if` for `DismissFinding`. Each arm sets `app.input_mode = InputMode::Normal` and calls `app.dismiss_finding_with_outcome(&finding_id, kind, "")`. On `Ok(true)` it clears `app.tab_mut().focused_finding_id` and calls `app.notify("Finding dismissed: <label>")`. On `Ok(false)` it calls `app.notify("Finding not found")`. On `Err(e)` it calls `app.notify(&format!("Dismiss failed: {e}"))`.
  - `crates/er-tui/src/ui/status_bar.rs:819`: add the prompt for `ConfirmAction::DismissFinding { .. }`. The match is exhaustive, so the build fails until you do.
  - `crates/er-tui/src/ui/status_bar.rs`: in each hint block that pushes `Hint::new("r", " reply ")` for `tab.focused_finding_id.is_some()` (near `:512`, `:618`, and the third block near `:671`), also push `Hint::new("x", " dismiss ")`.
  - `crates/er-engine/src/app/state/mod.rs:7337-7344`: change the help row description for `x` to `"Delete focused comment / dismiss focused finding"`.
- **Watch out for:** in confirm mode, `n` and `Esc` cancel and `k` means "keep" for agent prompts. Do not use those letters. `y` must do nothing for `DismissFinding`; the existing `y` chain falls through with no matching branch, so leave it.
- **Tests first** (in `crates/er-tui/src/main.rs` `mod tests`, next to `:792`, using `make_app`/`send_key`):
  - `bare_x_with_focused_finding_asks_for_outcome` — set `app.tab_mut().focused_finding_id = Some("f-1".into())`, send `x` → `input_mode == InputMode::Confirm(ConfirmAction::DismissFinding { finding_id: "f-1".into() })`.
  - `bare_x_closes_tab_when_no_comment_focused` (`:834`) must still pass unchanged.
  - In `crates/er-tui/src/input/mod.rs` `mod tests` (`:2026`): `w_in_dismiss_confirm_records_wrong`. Use an `App::new_for_test` with `ErRoot::RepoLocal(tmp)` and a `review.json` fixture as in Step 3. Set the confirm mode, call `handle_confirm_input` with `KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)`, then assert `er_engine::ai::load_finding_outcomes(&er_dir).outcomes[0].outcome == OutcomeKind::Wrong` and `input_mode == InputMode::Normal`.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui bare_x` → all three `x` tests pass. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui dismiss_confirm` → passes. Before the arm exists, the first new test fails, because `x` closes the tab and `input_mode` stays `Normal`.
- **If it fails:** if `focused_finding_id` is cleared before the key arm runs, check whether an earlier arm in the same `match` consumes `x` (`:136` handles Hidden mode only). Do not reorder unrelated arms.

## Gotchas & landmines
- **Branch guard on reload.** A fixture `review.json` whose `head_branch` names a real branch other than the bucket's makes `load_ai_state` return an empty `AiState` (`crates/er-engine/src/ai/CLAUDE.md`, "Traps"). Use `"unknown"` or omit it.
- **Prefixed ids.** Expert findings show as `sec-1` while the sidecar stores `1`. Look the finding up in the *loaded* review (the ids match there), and pass the same id to `remove_finding_from_sidecars`. It replays the prefix itself (`finding_cleanup.rs:44-58`).
- **`finding_key` includes the line.** An edit above a finding changes its key. That is accepted. Do not switch to a line-less key here; task 08 makes its own line-less key for no-repeats.
- **`cargo test -p er-desktop` compiles Tauri.** Run it last and only once.
- **No task numbers in code comments** (`just comments-check`). Write the reason, e.g. "kept after the finding is deleted so later passes can tell what the person said".

## Decision log (forks pre-answered)
- **Does dismiss still hide the finding?** Yes, by removing it from its sidecar as today. The outcome adds a record and changes nothing else.
- **Where does the file live?** In the active view bucket, beside `arbiter.json`. Per-project roll-ups are task 17's problem; it walks the buckets.
- **What if the review's diff hash has moved?** Record anyway, with the review's own `diff_hash`. Outcomes are history and are never filtered by staleness.
- **What if the same finding is dismissed twice?** Same `key` and `diff_hash` replaces the earlier record. A later review raising the same claim on a new diff appends a new record; that is the signal task 17 counts.
- **Does promote record an outcome?** No. Promotion is not one of the four answers. Task 08's ledger records it.
- **How task 08 uses this method (consistency pass, 2026-10-17):** 08 Step 5 adds one `ledger_mark` call inside `dismiss_finding_with_outcome`, before `remove_finding_from_sidecars`, keyed by its line-less `no_repeat_key`. Because both front ends dismiss through this method, that one hook covers the desktop and the TUI. Keep the method the single dismiss path so the hook stays in one place.
- **What about the reason in the TUI?** Empty. Confirm mode has no text field, and adding one is out of scope.
- **Engine method or desktop-only change?** Engine method. The TUI needs the same behaviour, and task 08 hooks one site.
- **Should outcomes join `AiState`?** No. Nothing reads them at load time yet. Whoever adds them later must also handle `finish_ai_reload` (`crates/er-engine/src/ai/CLAUDE.md`, "Traps").
- **Optional `outcome` on the Tauri command for old callers?** No, make it required. The two Svelte call sites are the only callers (`grep -rn dismiss_finding desktop-ui/src` at c96dc0f12).

## Full verification
1. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine outcomes` → 5 passed.
2. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine findings::tests` → 3 passed.
3. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui bare_x` and `… dismiss_confirm` → all pass.
4. `~/.claude/scripts/locked.sh . just test` → exits 0.
5. `~/.claude/scripts/locked.sh desktop-ui bun run test` → exits 0. `~/.claude/scripts/locked.sh desktop-ui bun run check` → 0 errors.
6. `~/.claude/scripts/locked.sh . cargo test -p er-desktop` → exits 0.
7. `~/.claude/scripts/locked.sh . just clippy` → exits 0. `~/.claude/scripts/locked.sh . just lint-ui` → exits 0.
8. `~/.claude/scripts/locked.sh . just docs-check` and `~/.claude/scripts/locked.sh . just comments-check` → both exit 0.
9. Manual: in the desktop, dismiss a finding as "Intentional" with reason "by design". It disappears. `cat "<er_dir>/finding-outcomes.json"` shows one record with `"outcome": "intentional"` and `"reason": "by design"`. Use "Reveal review files" in the AI Review card to find `<er_dir>`.

## Rollback
Revert the commits. `finding-outcomes.json` files already written are inert, because nothing else reads them until tasks 08, 09, 17 and 19 land. Leave them on disk.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- Task 08 has already landed with its own dismiss hook in `dismiss_finding`. Merge the two so both run from `dismiss_finding_with_outcome`; ask if they conflict.
- Someone asks for a fifth outcome (e.g. "promoted" or "duplicate"). That changes the stored enum, so it needs the person's approval.
- `just clippy` flags `findings.rs` or `outcomes.rs` for cohesion or size. Split along the groups the warning names; do not grow a baseline (ADR 0038).

## Comments
