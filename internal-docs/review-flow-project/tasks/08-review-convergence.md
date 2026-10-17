# 08 — Review has to converge

Status: needs-design
Idea: 3 in the original idea list (summarised in ../README.md)
Depends on: 03-finding-outcomes.md, 06-pre-existing-tag.md (07-default-review-flow.md is optional; see Step 6)
Impact: H · Complexity: H · Size: L
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.
>
> **This task is `needs-design`.** Do Step 1 (design note plus ADR draft), then stop and ask the person to approve it. Write no code before that approval. Steps 2 to 7 are the recommended build. Follow them as written unless the approved design changes them.

## In plain terms
- **The problem** — Each time er reviews a branch again, it starts from zero. It reads the whole diff, writes over the last review, and has no idea how many times it has already been round. It can raise a finding the person already dismissed. When an agent fixes the findings and er reviews again, the two can loop for as long as nobody stops them.
- **Why it matters** — er is built for an agent writing code while a person watches, and "Address notes" already hands findings back to an agent. That is the agent-author / agent-reviewer loop the blog warns about. Every extra round costs an agent run, a commit, and the person's attention. In the blog's data, most of what the reviewer flagged in later rounds was old code it missed the first time.
- **The fix** — er counts review rounds for each view bucket. After the first review, a re-run reviews only the files that changed since the last round. Findings still open carry forward, and fixed ones close. A finding the person already dismissed or promoted is never shown again. Once three rounds have not passed, er stops reviewing on its own and writes one "step back and reconsider the approach" item that lists the kinds of finding that kept coming back. A person can still ask for a full review of everything at any time.
- **TL;DR** — Make repeated reviews of one branch shrink and stop, instead of re-reading everything and looping.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> Review has to converge. An agent author and an agent reviewer can keep each other busy indefinitely. One CLI PR went through more than fifty rounds of review and fixes overnight before its author stopped it.

> When we ran git blame on what the reviewer kept flagging, about 59% of it was code that had been there since the reviewer's first pass.

> - A round cap. After three rounds, the reviewer asks the author to step back and reconsider the approach.
> - Diff-only re-review after the first few passes.
> - Flag the class of bug, so the author fixes every instance, not one line.
> - No repeats of findings already posted.

From Code Review docs, "What a later review covers" (https://code.claude.com/docs/en/code-review-pull-requests):

> A later review that starts on its own, such as after a push, focuses on what changed since the last review and posts fewer nits and pre-existing findings. Its summary counts what it left out, in a line that ends "found and not posted" or "outside the scope of this re-review and not posted."
>
> To get a review that leaves none of these out, comment `@claude review`. It covers the whole pull request again.

Figure D (blog): pass after one review-and-fix round 65% → 78%; five or more rounds 8.9% → 1.5%; mean reviews to pass 2.08 → 1.32.

## Mission & definition of done
Give each view bucket a round history that er writes and the agent never touches. Use it to make re-runs diff-only, to stop repeats, to cap rounds at three, and to record rounds-to-pass. Both front ends must reach it through one engine entry point. That entry point works with today's "Review work" action and later with task 07's flow.

- [ ] An ADR under `docs/adr/` records the round sidecar, the no-repeat identity, and the diff-only merge. The person has approved it.
- [ ] A host-written round sidecar exists in the active view bucket. `cleanup_reviews` and `cleanup_review_artifacts` do not delete it.
- [ ] A review that completes on a diff whose hash differs from the last round's hash appends a round. A review that completes on the same diff hash replaces the last round instead.
- [ ] With a review present and the diff moved, the default re-run reviews only files whose per-file diff hash changed or that are new. Findings on unchanged files carry forward. The merged review is not stale against the current full diff.
- [ ] A full re-run requested by hand reviews the whole diff, as it does today.
- [ ] A finding whose no-repeat key matches a dismissed or promoted finding is hidden at load time. The UI shows the count in a line that ends "found and not posted".
- [ ] When a fourth round is due and none of the first three passed, er spawns no review. It writes one step-back item listing the recurring finding patterns, and both front ends show it.
- [ ] Pre-existing findings (task 06) never make a round count as failed and never appear in the step-back list.
- [ ] Rounds-to-pass is stored in the round sidecar.
- [ ] The review prompt asks for a defect pattern and every site where it occurs. The fields round-trip through serde with defaults, so old sidecars still load.
- [ ] TUI and desktop both show the round number and the "found and not posted" count.

**Risk:** high. This changes what the review shows (findings can be hidden) and adds stored state that must survive "clear previous". A wrong suppression hides a real finding without anyone seeing it.

## Before you start (orientation)
- **Branch:** see `../README.md` "Branching". This is a feature: use the current release branch.
- **Commands:**
  - Engine tests: `~/.claude/scripts/locked.sh . cargo test -p er-engine <filter>`
  - TUI tests: `~/.claude/scripts/locked.sh . cargo test -p er-tui <filter>`
  - Desktop backend: `~/.claude/scripts/locked.sh . cargo test -p er-desktop <filter>`
  - Desktop UI: `~/.claude/scripts/locked.sh . just test-ui`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`, `just docs-check`, `just comments-check`
- **The map** (all paths confirmed at c96dc0f12):
  - `crates/er-engine/src/ai/identity.rs:28` — `pub fn finding_key(file: &str, line: Option<usize>, text: &str) -> String`. Re-exported by `crates/er-engine/src/ai/mod.rs` (`pub use identity::*;`). `arbiter.rs:163` only calls it.
  - `crates/er-engine/src/ai/scoped_merge.rs` — the selected-files run. Manifest `review-files.txt`, `snapshot_before_scoped_review` (line 37), `apply_scoped_sidecar_merge` (line 93), `merge_scoped_review` (line 121). The diff-only round reuses this path.
  - `crates/er-engine/src/ai/loader.rs:57` — `pub fn compute_per_file_hashes(raw_diff: &str) -> HashMap<String, String>`. `load_ai_state` is at line 208. Staleness is `state.is_stale = review.diff_hash != current_diff_hash;` (about line 226). The arbiter overlay runs at about line 355.
  - `crates/er-engine/src/ai/arbiter.rs:65` — `load_arbiter_review` / `write_arbiter_review` (line 76). The pattern to copy for a host-written sidecar.
  - `crates/er-engine/src/ai/review.rs:70` — `ErReview` (has `diff_hash`, `files`, `file_hashes`). `Finding` is at line 271. `lenient_findings` is at line 120. `AiState` is at line 732.
  - `crates/er-engine/src/ai/prompts.rs` — `general_review_json_example` (line 186), `general_review_outputs_section` (line 216, it tells the agent to write `"file_hashes": {}`), `build_review_prompt_prepared_diff` (line 451), `file_scope_appendix` (line 824), validate prompts (lines 1357–1453).
  - `crates/er-engine/src/app/state/mod.rs` — `cleanup_reviews` (line 9023), `cleanup_review_artifacts` (line 9037), `compute_stale_files` (line 4181, returns early because `file_hashes` is always empty), `finish_ai_reload` (line 3760), `open_ai_hub` "Review work" entry (about line 6636), `ConfirmAction::RunAgentReview` (line 355).
  - `crates/er-engine/src/app/state/comments.rs` — `check_commands` (line 2393, TUI `spawn_agent_prompt` completions); `spawn_background_review` (line 2808); the worker closure that calls `apply_scoped_sidecar_merge` (about line 3554, desktop background completions).
  - `crates/er-tui/src/input/mod.rs` — `HubAction::PromptReview` (about line 405), confirm `y` branch (about line 819, calls `cleanup_reviews` then `start_agent_review`), confirm `k` branch (about line 880), `start_agent_review` (line 1037), `build_agent_review_prompt` (about line 1050).
  - `crates/er-desktop/src/commands.rs` — `list_review_revisions` (about line 2362, comment "Branch-level managed storage keeps no revisions"), `run_ai_review` (line 3825), `run_ai_scoped_review` (about line 4283), `dismiss_finding` (line 9020, removal at 9033), `promote_finding_to_comment_impl` (line 9065, removal at about 9140).
  - `crates/er-engine/src/git/diff.rs:169` — `pub fn filter_raw_diff_by_paths(raw: &str, paths: &[String]) -> String`.
  - `crates/er-tui/src/ui/panel.rs` — `render_ai_summary` (line 679), `push_arbiter_effect` (line 484, the pattern for a count line).
  - `desktop-ui/src/lib/components/TriageCard.svelte`, `desktop-ui/src/lib/triageCard.ts` (line 113 issues `run_ai_review`).
- **Conventions to follow** — host-written sidecar, atomic write (`crates/er-engine/src/ai/arbiter.rs:76`):

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

  Load-time overlay (`crates/er-engine/src/ai/loader.rs`, about line 353):

  ```rust
  // Overlay arbiter verdicts last, so they grade every finding the merges
  // above put into the review rather than only the review's own.
  if let Some(arbiter) = super::arbiter::load_arbiter_review(er_dir) {
      if let Some(review) = state.review.as_mut() {
          state.arbiter_effect = super::arbiter::merge_arbiter_into_review(review, &arbiter);
      }
  }
  ```

  Merge test helpers to reuse: `finding(id, title)` and `review_with(files, hash)` in the `tests` module of `crates/er-engine/src/ai/scoped_merge.rs` (about lines 298–350).
- **Mental model** — An agent writes `review.json` into the active view bucket. er reads it back through `load_ai_state`, which overlays expert, professor and arbiter sidecars. A review is stale when its `diff_hash` differs from the current diff's SHA-256. Today a re-run overwrites `review.json`; nothing records earlier rounds. The selected-files run already reviews a subset of paths: it writes `review-files.txt`, snapshots `review.json` to `review.prev.json`, and merges on exit. That merge keeps the *previous* `diff_hash` on purpose. Dismiss and promote both delete the finding from its sidecar (ADR 0009). After that, no record of the finding remains.
- **ADRs to read first:**
  - 0003 — sidecars resolve through `storage.rs`; never build a bucket path by hand.
  - 0004 — artifacts are per view bucket; the round history is too.
  - 0009 — findings are AI-owned; er may hide or remove one, never rewrite its text.
  - 0010 — finding staleness is the diff hash; the diff-only merge must keep that meaning.
  - 0011 — the desktop is push-driven; a new sidecar change must reach the UI through the existing reload and revision path.
  - 0021 — a new spawn path must take an agent slot.
  - 0035 — overlays beside `review.json` instead of a fourth writer; why `finding_key` carries the line.

## Scope boundaries
- **In scope:** the round sidecar; round counting and rounds-to-pass; diff-only re-review on top of the selected-files path; the no-repeat overlay; the step-back item; defect pattern and sites fields plus the prompt text that asks for them; showing round number, "found and not posted", and the step-back item in the TUI and the desktop; one ADR; `CONTEXT.md` terms.
- **OUT of scope / DO NOT TOUCH:**
  - Task 07's review flow itself (triage, Find, Verify, Rank). Only call into it as Step 6 says.
  - Recording finding outcomes (task 03). Read its record; do not build it.
  - The `pre_existing` flag itself (task 06). Read it; do not build it.
  - Several rounds only on risky PRs (part of the original idea 3: "Several rounds only where risk earns it"). It needs task 12's People needed label.
  - Showing rounds-to-pass as a metric or eval (task 19). Store it only.
  - er-mcp (`crates/er-mcp`). It starts no agents, so it has no rounds to count.
  - Automatic re-runs. "Decisions already made" in `../README.md` says re-runs after new commits are offered, never started on their own.
  - Arena import (`arena/import.rs`) and the arbiter's own files.
  - Changing `finding_key` itself. Arbiter verdicts depend on its line-in-key behaviour (ADR 0035).

## Steps

### Step 1 — Design note and ADR draft (stop for approval)
- **Goal:** A design the person approves before any code is written.
- **Why / invariant:** The round history is new stored state that must outlive a review being cleared, and hiding findings is hard to see when it goes wrong. ADR 0035 already chose overlays to keep `review.json` at three writers; this design must not add a fourth.
- **Files:** new `docs/adr/NNNN-review-rounds-converge.md`. Take the next free number when you start; 0044 was free at c96dc0f12, but tasks 05, 06 and 07 also add ADRs. Read `docs/agents/writing-docs.md` first.
- **Do this:** Write the ADR with these decisions. Each has a recommendation; keep it unless you find a reason in the code, and say so if you change one.
  1. **Where round state lives.** Recommend a new host-written sidecar `rounds.json` in the active view bucket (`tab.er_dir()`), with a load and an atomic-write function in a new `crates/er-engine/src/ai/rounds.rs`, copied from `arbiter.rs:65-83`. Only er writes it; no agent prompt names it. `cleanup_reviews` and `cleanup_review_artifacts` must not delete it. Suggested shape:
     ```json
     {
       "version": 1,
       "rounds": [
         {
           "n": 1,
           "kind": "full",              // full | changes_only | full_by_hand
           "diff_hash": "<sha256 of the full diff>",
           "head_oid": "<git HEAD at start, or empty>",
           "finished_at": "<ISO 8601>",
           "file_hashes": { "src/a.rs": "<sha256 of that file's diff section>" },
           "raised": ["<no-repeat key>", "..."],
           "blocking_open": 2,
           "passed": false,
           "not_posted": 3
         }
       ],
       "ledger": { "<no-repeat key>": { "state": "dismissed", "pattern": "...", "round": 1 } },
       "rounds_to_pass": null,
       "step_back": null
     }
     ```
     Alternative considered: keep revisions of `review.json` (the comment at `crates/er-desktop/src/commands.rs:2364` says branch storage keeps none). Rejected: it adds a writer to `review.json` and copies whole reviews to answer a question that needs only keys and counts.
  2. **No-repeat identity.** `finding_key` (`identity.rs:28`) includes the line. A fix commit above a finding moves the line and changes the key, so the same claim would look new. ADR 0035 accepted that for verdicts because failing to match is the safe direction there. For suppression it is the unsafe direction, because repeats get through. Finding `id`s (`f-1`, `sec-1`) are positional and unusable. Recommend `finding_key(path, None, &finding.title)`, a key with no line. That function already produces this key for line-less findings (see test `finding_key_without_a_line_differs_from_line_one`), so no new hash is needed. Accepted failure: two different findings with the same title in one file share a no-repeat key, so a dismissed one hides the other. Record that, and that paraphrased titles are not caught (`canonical_finding_text` folds case and whitespace only). The prompt list in Step 4 covers paraphrase. Option considered: relocating anchors through `ai/relocate.rs` (`relocate_comment`). Rejected for v1: it needs the old anchor's context lines, which findings do not keep.
  3. **What a repeat is.** Recommend:
     - Key in the ledger as `dismissed` or `promoted` → hidden, counted in `not_posted`.
     - Key was raised in an earlier round and is still open → kept as one row. This is the open finding carried forward, so it is not a new raise.
     - Key was closed as fixed in an earlier round and comes back → shown, marked as recurring, and its pattern counts toward the step-back list. A fix that did not hold is what the step-back item is for.
  4. **What a round is and when it passes.** A round is a review run that completes successfully. It appends a round when its full-diff hash differs from the last round's, and replaces the last round when the hash is the same (no fix happened in between). A finding is *blocking* when it is open, its `pre_existing` flag is false (06 Step 2), it is not `Confidence::Dropped`, not hidden by the arbiter overlay, and its severity is `High` or `Medium`. `High`/`Medium` is the Important side of the mapping in 06's Decision log; no task adds an Important field, so keep this severity rule. Task 09's "must fix" count uses the same rule. A round passes when it has no blocking findings. `rounds_to_pass` is the `n` of the first passing round. Pre-existing findings never fail a round and never join the step-back list.
  5. **Diff-only merge.** `merge_scoped_review` keeps `previous.diff_hash` on purpose, for a selected-files run that leaves the rest of the diff where it was. After new commits the full diff has moved, so the merged review would read as stale (`loader.rs` `is_stale`), and `merge_arbiter_into_review` would refuse every verdict (`arbiter.rs`, the `diff_hash` check at the top of the function). Recommend a sibling `merge_changes_only_review(previous, scoped, changed_paths, full_diff_hash)` that does the same per-file swap and then stamps `full_diff_hash`. This is correct because every file outside `changed_paths` has a byte-identical diff section, so its findings and line numbers still hold.
  6. **Which files a changes-only round covers.** Recommend files whose `compute_per_file_hashes` value differs from the last round's `file_hashes`, plus files absent from it. File hashes are computed by er at round end, never by the agent. The agent writes `"file_hashes": {}` today (`prompts.rs:216`), so `ErReview.file_hashes` is empty and cannot be used. This works on uncommitted changes too, which task 14 needs. `head_oid` is stored for display only.
  7. **Step-back item.** Stored as `step_back` in `rounds.json`: `{created_at, after_round, patterns: [{pattern, count, rounds}]}`. It is shown as its own card, never written into `review.json` (that would be a fourth writer). It is not a Finding, because it is not about one place in the diff. It is not a Note, because notes are the person's (CONTEXT.md).
  8. **Defect pattern and sites.** `Finding` has only `related_files: Vec<String>`. Recommend two new optional fields, `pattern: String` (a short name for the kind of defect, finer than Category) and `sites: Vec<FindingSite { file, line_start }>`. `evidence` means "what I read", so it is the wrong place for "where else it is wrong". Do not call the field `class`: CONTEXT.md lists "class" under *Avoid* for Tier.
  9. **Bucket.** The round history is per view bucket, so on a local PR tab the branch view and the PR diff view count separately (ADR 0004). This is the default; route it to the person (see "When to STOP and ask").
  10. **Full re-run by hand.** It reviews everything, ignores the cap, and still appends a round of kind `full_by_hand`. It does not clear the ledger. Dismissed findings stay hidden even on a full run, matching the original idea list's rule that a dismissed finding is "never raised again".

  Also add `CONTEXT.md` entries for **Round** (one review of a view bucket's diff, counted toward the cap) and **Defect pattern**, each with an *Avoid* line. Use plain words in the ADR; cite this file's Source quotes.
- **Watch out for:** writing any code. Only the ADR and the `CONTEXT.md` lines belong in this step.
- **Verify:** `just docs-check` → exit 0.
- **If it fails:** fix the link or reference it names; do not add an inventory table.
- **Then STOP** and ask the person to approve the ADR. Record the answer under `## Comments` in this file.

### Step 2 — `rounds.rs`: the sidecar, keys and round recording (engine, tests first)
- **Goal:** A pure engine module that loads and saves `rounds.json`, computes no-repeat keys, and records a round from a loaded review.
- **Why / invariant:** One writer (er), atomic writes, and the file survives `cleanup_reviews`. Key computation must match the approved ADR exactly, because the overlay in Step 3 and the dismiss and promote hooks in Step 5 must agree.
- **Files:** new `crates/er-engine/src/ai/rounds.rs`; `crates/er-engine/src/ai/mod.rs` (add `pub mod rounds;` and `pub use rounds::*;` beside `pub mod arbiter;` / `pub use arbiter::*;`).
- **Do this:**
  - Types `ReviewRounds`, `Round`, `RoundKind { Full, ChangesOnly, FullByHand }`, `LedgerEntry { state: LedgerState { Raised, Dismissed, Promoted, Fixed }, pattern: String, round: u32 }`, `StepBack`. Derive `Serialize, Deserialize, Default` with `#[serde(default)]` on every field after `version`, as `ArbiterReview` does.
  - `pub fn no_repeat_key(path: &str, finding: &Finding) -> String { super::identity::finding_key(path, None, &finding.title) }`.
  - `pub fn load_rounds(er_dir: &str) -> ReviewRounds` (absent or malformed → default) and `pub fn write_rounds(er_dir: &str, r: &ReviewRounds) -> anyhow::Result<()>` (copy `write_arbiter_review`).
  - `pub fn record_round(er_dir: &str, review: &ErReview, full_raw_diff: &str, kind: RoundKind) -> anyhow::Result<Round>`. It computes `diff_hash` with `compute_diff_hash` and `file_hashes` with `compute_per_file_hashes`. It collects raised keys and blocking count. It marks earlier-round `Raised` keys that are now absent from changed files as `Fixed`, then appends or replaces a round (Step 1, item 4) and sets `rounds_to_pass` the first time a round passes.
  - `pub fn ledger_mark(er_dir: &str, key: String, state: LedgerState, pattern: String) -> anyhow::Result<()>`, used by Step 5.
  - `pub fn is_blocking(f: &Finding) -> bool`. For the Pre-existing check, read `f.pre_existing`, the `pub pre_existing: bool` field (`#[serde(default)]`) that 06-pre-existing-tag.md Step 2 adds to `Finding` in `crates/er-engine/src/ai/review.rs`. If `grep -n "pub pre_existing: bool" crates/er-engine/src/ai/review.rs` finds nothing, task 06 has not landed: stop (it is a listed dependency).
  - Task 14 later adds a `RoundKind::Live` variant and an `origin` field on `LedgerEntry` (14 Step 5). Write the cap check in `plan_next_round` as "count rounds whose kind is `Full`, `ChangesOnly` or `FullByHand`" rather than "count all rounds", so a later kind is ignored by default.
- **Tests first** (inline `#[cfg(test)] mod tests` in `rounds.rs`, using `tempfile::tempdir()` as `scoped_merge.rs` `apply_merge_on_disk_preserves_siblings` does):
  - `record_round_appends_on_new_hash_and_replaces_on_same_hash`
  - `round_with_only_pre_existing_findings_passes`
  - `rounds_to_pass_is_first_passing_round`
  - `finding_absent_from_changed_file_is_marked_fixed`
  - `no_repeat_key_ignores_line` (same title, lines 10 and 40 → equal keys)
  - `rounds_file_survives_cleanup_reviews` (write `rounds.json`, call `crate::app::cleanup_reviews` and `cleanup_review_artifacts`, assert the file still exists. Confirm the import path with `grep -n "pub fn cleanup_reviews" crates/er-engine/src/app/state/mod.rs` and the `app` re-exports.)
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine rounds` → all new tests pass. Before writing the bodies, run once with `todo!()` bodies → the tests fail.
- **If it fails:** a test that cannot see `cleanup_reviews` needs the right path. Do not move the function.

### Step 3 — No-repeat overlay at load, and "found and not posted"
- **Goal:** Hide ledger-dismissed and ledger-promoted findings at load time, mark recurring ones, and count what was hidden.
- **Why / invariant:** ADR 0009: er hides or removes, never rewrites text. ADR 0035: an overlay beside `review.json`, never a write into it. Every consumer reads through `load_ai_state`, so the overlay belongs there.
- **Files:** `crates/er-engine/src/ai/loader.rs` (after the arbiter overlay, about line 355); `crates/er-engine/src/ai/review.rs` (`AiState`, line 732: add `pub rounds: super::rounds::ReviewRounds` and `pub repeat_effect: super::rounds::RepeatEffect { hidden: usize, recurring: usize }`).
- **Do this:** In `load_ai_state`, after the arbiter overlay, `let rounds = super::rounds::load_rounds(er_dir);` then `state.repeat_effect = super::rounds::apply_no_repeat_overlay(review, &rounds);`. Hide by setting the finding's `confidence` to `Confidence::Dropped`. The UI already hides and lists `Dropped` (see the `arbiter_effect` doc comment in `AiState`). Mark recurring with a new `#[serde(skip)] pub recurring: bool` on `Finding`, runtime only. The field is skipped by serde, so it is not part of the sidecar. Then assign `state.rounds = rounds`.
- **Watch out for:** `finish_ai_reload` (`app/state/mod.rs:3760`) carries some fields across a reload by hand. `rounds` and `repeat_effect` are rebuilt by `load_ai_state` on every reload, so they need no carry. Confirm by reading `reload_ai_state`.
- **Tests first** (inline in `rounds.rs`): `dismissed_key_is_hidden_and_counted`, `promoted_key_is_hidden_and_counted`, `open_raised_key_is_kept_once`, `fixed_key_that_returns_is_shown_and_marked_recurring`.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine rounds` → pass; `~/.claude/scripts/locked.sh . cargo test -p er-engine loader` → no regressions.

### Step 4 — Diff-only round: merge, prompt and engine entry point
- **Goal:** One engine function both front ends call to start a review. It decides between a full run, a changes-only run, and the step-back item.
- **Why / invariant:** ADR 0006: engine state is the UI contract, so neither front end may decide the round logic itself. The merged review must not read as stale (ADR 0010).
- **Files:** `crates/er-engine/src/ai/scoped_merge.rs` (new `merge_changes_only_review` beside `merge_scoped_review`, line 121); `crates/er-engine/src/ai/prompts.rs` (new appendix beside `file_scope_appendix`, line 824; `general_review_json_example`, line 186); `crates/er-engine/src/ai/review.rs` (`Finding`: `pattern`, `sites` with `#[serde(default)]`); new `crates/er-engine/src/ai/rounds.rs` function `plan_next_round`.
- **Do this:**
  - `pub fn plan_next_round(er_dir: &str, full_raw_diff: &str, by_hand_full: bool) -> RoundPlan` with `enum RoundPlan { Full, ChangesOnly { paths: Vec<String> }, StepBack(StepBack), NothingChanged }`. Rules:
    - `by_hand_full` → `Full`.
    - No rounds → `Full`.
    - Three or more rounds counted and none passed → `StepBack`, built from ledger entries whose pattern appears in two or more rounds, excluding Pre-existing.
    - Changed paths empty → `NothingChanged`.
    - Otherwise → `ChangesOnly`.
  - `merge_changes_only_review(previous, scoped, paths, full_diff_hash)` (Step 1, item 5). On exit, `apply_scoped_sidecar_merge` must call it instead of `merge_scoped_review` when a changes-only run wrote a marker. Recommend a second manifest line or a sibling file `review-round.txt` containing `changes_only <full_diff_hash>`. Remove the marker in `clear_scoped_review_snapshots`.
  - Prompt appendix `changes_only_appendix(output_dir, prior: &[(path, title, pattern)])`: lists the prior open findings on the changed files and the ledger titles not to raise again. It asks the agent to keep each prior finding that still applies (re-anchored), drop the fixed ones, and add new findings only on changed lines. Keep the preamble's anchor rule as task 06 left it: 06 Step 3 builds the anchor block from `FindingCaps.pre_existing`, so the general and expert prompts allow a capped Pre-existing finding on a tagged context line. The appendix adds no anchor rule of its own. Per the Code Review docs quote in Source, a later round raises fewer Pre-existing findings; Step 1 decides whether the appendix lowers that cap (recommend: leave the cap, and rely on the ledger to stop repeats).
  - In `general_review_json_example`, add `"pattern": ""` and `"sites": []`, plus one instruction line: name the defect pattern and list every site you saw, so the fixing agent fixes them all in one pass.
- **Tests first:**
  - In `scoped_merge.rs` tests: `changes_only_merge_stamps_full_diff_hash`, `changes_only_merge_keeps_unchanged_files`.
  - In `rounds.rs`: `plan_is_full_with_no_rounds`, `plan_is_changes_only_for_moved_files`, `plan_is_step_back_after_three_failed_rounds`, `plan_by_hand_is_full_even_after_three`, `plan_step_back_skips_pre_existing_patterns`.
  - In `prompts.rs` tests (existing module; see `file_scope_appendix_mentions_review_files`, about line 2514): `changes_only_appendix_lists_prior_titles`.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine scoped_merge` and `... rounds` and `... prompts` → pass.
- **If it fails:** if `apply_scoped_sidecar_merge` cannot tell a changes-only run from a selected-files run, stop and ask. Do not change `merge_scoped_review`'s hash rule; selected-files runs rely on it.

### Step 5 — Record dismiss, promote and round completion
- **Goal:** The ledger learns about every dismiss and promote, and a round is recorded when a review completes.
- **Why / invariant:** Dismiss and promote delete the finding (ADR 0009). After that, no copy remains to take a key from, so the key must be written *before* the deletion.
- **Files:**
  - **Dismiss (both front ends, one site):** task 03 Step 3 moves every dismissal into `App::dismiss_finding_with_outcome(&mut self, finding_id, outcome: ai::OutcomeKind, reason)` in `crates/er-engine/src/app/state/findings.rs`. The desktop `dismiss_finding` command (03 Step 4) and the TUI `x` key (03 Step 6, `ConfirmAction::DismissFinding`) both call it. Add the ledger write inside that method, after the finding lookup and before `ai::remove_finding_from_sidecars`: `ai::ledger_mark(&er_dir, ai::no_repeat_key(path, f), LedgerState::Dismissed, f.pattern.clone())` for every outcome, so a dismissed finding is never shown again. Whether a "Fixed it" answer should mark `LedgerState::Fixed` instead is an open owner question (Step 1 product decisions, and the Decision log); build the default unless the approved ADR says otherwise. Do not hook the desktop command separately; 03's STOP list asks for one hook in the engine method.
  - **Promote (desktop only):** about `crates/er-desktop/src/commands.rs:9140` (`promote_finding_to_comment_impl`). Before `remove_finding_from_sidecars`, look up the finding in `app.tab().ai.review` by id and path, then call `er_engine::ai::ledger_mark(&er_dir, no_repeat_key(path, f), LedgerState::Promoted, f.pattern.clone())`. Task 03 records no outcome on promote (03 Decision log), so this is the only record of it. The TUI had no promote path at c96dc0f12 (`grep -n promote crates/er-tui/src/input/mod.rs` returned nothing).
  - Round completion:
    - TUI path: `check_commands` (`crates/er-engine/src/app/state/comments.rs:2393`), in the `Ok(())` arm where `name == "review"`, before `tab.reload_ai_state()`.
    - Desktop path: the worker closure after `apply_scoped_sidecar_merge` (about `comments.rs:3554`) when `command_name_fail == "review"`.

    Each path loads the review with `load_ai_state(er_dir, …)` and calls `record_round`. It needs the full raw diff: `TODO(executor): confirm how the worker reaches the full diff — search for "raw_diff_for_review" in crates/er-engine/src/app/state/`. If the worker cannot reach it, store the full diff's per-file hashes in the marker file at spawn time (Step 4) and read them back.
- **Tests:** in the `#[cfg(test)] mod tests` that 03 Step 3 adds to `crates/er-engine/src/app/state/findings.rs`, beside `dismiss_records_outcome_then_removes_finding`, add `dismiss_marks_the_ledger` (dismiss as `Wrong` → `load_rounds(&er_dir).ledger` has the finding's `no_repeat_key` as `Dismissed`) and `dismiss_as_fixed_marks_the_ledger_dismissed` (the default; change it only if the approved ADR maps "Fixed it" to `Fixed`). Put a `ledger_mark` round-trip test in `rounds.rs`. Test the promote hook by hand in Full verification.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine rounds` → pass; `~/.claude/scripts/locked.sh . cargo test -p er-engine findings::tests` → pass, including 03's three tests unchanged.

### Step 6 — Wire both front ends to `plan_next_round`
- **Goal:** The existing Review actions use the round plan. Task 07's flow uses it later through the same call.
- **Why / invariant:** Re-runs are offered, never started on their own (README "Decisions already made"). A full re-run by hand must stay one keypress or click away.
- **Without task 07 (today's "Review work" action):**
  - TUI: `HubAction::PromptReview` (`crates/er-tui/src/input/mod.rs`, about line 405) already asks when a review exists. Keep the dialog and change what its keys do:
    - `k` ("keep previous") → call `plan_next_round(er_dir, raw, false)`. `ChangesOnly` → write `review-files.txt`, call `snapshot_before_scoped_review`, prepare the filtered diff with `filter_raw_diff_by_paths`, add the appendix, and spawn `"review"`. `StepBack` → `write_rounds` with the step-back item and `notify`. `NothingChanged` → `notify("Nothing changed since the last round")`.
    - `y` ("clear") → full run by hand, as today. It calls `cleanup_reviews`, which leaves `rounds.json` in place (Step 2 test).

    Update the confirm text in `crates/er-tui/src/ui/status_bar.rs:831` (`ConfirmAction::RunAgentReview`) to say "k: review changes only · y: clear and review everything".
  - Desktop: `run_ai_review` (`crates/er-desktop/src/commands.rs:3825`) gains `full: Option<bool>`. `None`/`false` → `plan_next_round`, and for `ChangesOnly` it follows the scoped branch of `run_ai_scoped_review` (about line 4330). `true` → today's behaviour. In `desktop-ui`, the existing callers (`triageCard.ts:113`, `ChecklistCard.svelte:69`, `CommandPalette.svelte:140`) keep the default. Add one "Review everything again" command in `CommandPalette.svelte` that passes `full: true`. Register any new argument the way `run_ai_review` is registered today; `TODO(executor): confirm the permission/registration list — search for "run_ai_review" in crates/er-desktop/permissions/ and crates/er-desktop/src/main.rs`.
- **With task 07:** 07 Step 5 defines the flow's entry point `App::start_review_flow(&mut self, scope: &str)` and its driver `App::advance_review_flows(&mut self)`, both in `crates/er-engine/src/app/state/review_flow.rs`; a run ends when the reducer (`crate::ai::ReviewFlow::apply`, 07 Step 2) moves to `FlowState::Done` after `FlowEvent::VerifyDone`. 07 has no separate "Rank" stage: Rank is the seeded dedupe plus the arbiter overlay. Wire it this way:
  - In `start_review_flow`, after the raw diff is built and before triage spawns, call `plan_next_round(&er_dir, &raw, false)`. `StepBack` → `write_rounds` with the step-back item, `notify`, and start no flow. `NothingChanged` → `notify` and start no flow. `ChangesOnly { paths }` → write `review-files.txt`, call `snapshot_before_scoped_review`, and have the flow's finders use the filtered diff and `changes_only_appendix`. `Full` → as 07 wrote it.
  - In `advance_review_flows`, when a flow first reaches `FlowState::Done`, call `record_round` with the round kind the plan chose.
  - Give 07's Review entry a "Review everything again" companion (TUI hub item, desktop palette command) that passes `by_hand_full: true`.
- **Tests:** TUI input tests near the existing `PromptReview` tests. `TODO(executor): confirm location — search for "RunAgentReview" under #[cfg(test)] in crates/er-tui/src/`. Add `keep_on_moved_diff_writes_review_files_manifest`.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-tui review` → pass; `~/.claude/scripts/locked.sh . cargo test -p er-desktop` → pass.

### Step 7 — Show round, "found and not posted", and step-back
- **Goal:** The person sees which round this is, how much was held back, and the step-back item.
- **Files:**
  - TUI: `crates/er-tui/src/ui/panel.rs` `render_ai_summary` (line 679). Add a line in the style of `push_arbiter_effect` (line 484): `Round 2 (changes only) · 3 found and not posted`. Show the step-back item as a block that lists each pattern with its count.
  - Desktop: snapshot field in `crates/er-desktop/src/snapshot.rs` beside `pub triage: Option<TriageSnapshot>` (about line 1248); render it in `desktop-ui/src/lib/components/TriageCard.svelte` and the summary area. Use theme tokens (ADR 0030); no new colours.
- **Watch out for:** ADR 0006. Add an explicit snapshot field; never reuse an existing one.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-desktop snapshot` → pass; `~/.claude/scripts/locked.sh . just test-ui` → pass; `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.

## Gotchas & landmines
- `merge_scoped_review` keeps the old `diff_hash` on purpose. Reusing it for changes-only rounds makes every merged review stale and switches off the arbiter overlay. Use the new sibling.
- `ErReview.file_hashes` is empty in practice, because the prompt tells the agent to write `{}`. Never read it for round logic.
- `finding_key` with a line will not match across a fix commit. Use `no_repeat_key`.
- `cleanup_reviews` runs on the TUI "clear" path. A round file named like a review sidecar, or placed in a subfolder that `cleanup_review_artifacts` sweeps (`experts/`), is lost.
- The loader's branch guard empties `AiState` when `review.json` names a different branch (`ai/CLAUDE.md` "Traps"). `rounds.json` loads inside that guard, so it empties too. That is the right outcome.
- `just comments-check` rejects comments that cite task numbers. Write the reason in comments, never "task 08".
- Hiding by `Confidence::Dropped` changes the gate (`min_trust_for`, `review.rs:35`) only if the arbiter has graded. Check that the hidden findings do not move the gate. If they do, add a separate hidden flag and stop to ask.

## Decision log (forks pre-answered)
- **Ledger versus task 03's record:** 03's `finding-outcomes.json` keys records with `finding_key`, which includes the line, so it cannot serve as the no-repeat ledger. Write `ledger_mark` inside 03's `dismiss_finding_with_outcome` (Step 5) and keep both records. Do not build or change task 03's outcome UI or format.
- **A person's "Fixed it" answer (consistency pass, 2026-10-17):** 03's `OutcomeKind::Fixed` means the person says they fixed it; this file's `LedgerState::Fixed` means the claim went away. Default: every dismissal, "Fixed it" included, marks `LedgerState::Dismissed`, so a dismissed finding is never shown again (In plain terms, the no-repeat DoD item). The alternative, mapping `OutcomeKind::Fixed` to `LedgerState::Fixed` so a returning claim shows as recurring (a fix that did not hold), is the owner's call; put it in the Step 1 ADR.
- **If task 06's `pre_existing` flag is missing:** stop. It is a listed dependency, and the cap rule cannot be written without it.
- **If task 07 is not built:** do Step 6's "Without task 07" part only and leave a `## Comments` note.
- **If the agent re-raises a prior finding under a slightly different title:** it shows as new. Accept this in v1; the prompt list in Step 4 is the mitigation.
- **If a diff-only round produces no `review.json` change at all:** still record the round. It passes if no blocking findings remain.
- **If a desktop and a TUI instance write `rounds.json` at once:** the atomic rename keeps the file whole and the last write wins. CLAUDE.md allows one `er` per worktree, so accept this.
- **The field is named "pattern":** CONTEXT.md lists "class" under *Avoid* for Tier. Task 15 adds an unrelated triage block named `pattern_setting` (changes that set a pattern others will copy). Keep the two apart in names and prose: this file's word is "defect pattern".
- **Names later tasks read from this file (consistency pass, 2026-10-17).** Keep them, or record renames under `## Comments`.
  - 09 Step 6: `ReviewRounds.step_back` (via `AiState.rounds`) and `plan_next_round`.
  - 14 Steps 5–6: `RoundKind`, `LedgerEntry`, `plan_next_round`, `changes_only_appendix`; 14 adds `RoundKind::Live` and `LedgerEntry.origin`.
  - 15 Step 7: `AiState.repeat_effect.hidden` is the "found and not posted" count.
  - 17 Step 1: `Finding.pattern`.
  - 19 Step 2: `rounds.json` and `rounds_to_pass`.

## Full verification
1. `~/.claude/scripts/locked.sh . cargo test -p er-engine rounds` → all pass.
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine scoped_merge` → all pass, including the two new tests.
3. `~/.claude/scripts/locked.sh . cargo test -p er-engine` → pass.
4. `~/.claude/scripts/locked.sh . cargo test -p er-tui` → pass.
5. `~/.claude/scripts/locked.sh . cargo test -p er-desktop` → pass.
6. `~/.claude/scripts/locked.sh . just test-ui` → pass.
7. `~/.claude/scripts/locked.sh . just clippy` → exit 0; `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
8. `just docs-check` and `just comments-check` → exit 0.
9. By hand, in a scratch repo with `just run`:
   - Review work → round 1.
   - Edit one file and press `k` → only that file is reviewed, and the panel shows `Round 2 (changes only)`.
   - Dismiss a finding as "Wrong" (desktop or TUI `x` then `w`), make an edit that keeps it, run again → it does not reappear, and "1 found and not posted" shows.
   - After three failing rounds, `k` → no agent starts, and the step-back item appears.
   - `y` → full review runs.

## Rollback
Revert the commits. `rounds.json` files left in buckets are ignored by older builds: nothing reads them, and `load_ai_state` does not fail on unknown files. The new `Finding` fields are `#[serde(default)]`, so reviews written by the new prompt still load in older builds (serde ignores unknown fields unless `deny_unknown_fields` is set; confirm `Finding` does not set it).

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- After Step 1, always: the ADR needs the person's approval.
- Product decisions to put to the person in Step 1, with this file's defaults:
  - Does a full re-run by hand after three failed rounds start straight away (default) or ask first?
  - Is the round count per view bucket (default; the branch view and PR diff view count separately) or shared across the two views of one PR?
  - Should a full re-run by hand clear the dismissed ledger (default: no)?
  - Does a person's "Fixed it" dismissal (03's `OutcomeKind::Fixed`) mark the ledger `Dismissed`, so a return is hidden (default), or `Fixed`, so a return shows as recurring?
  - Does the changes-only appendix lower the Pre-existing cap for later rounds (default: no; the ledger stops repeats)?
- Hiding findings moves the confidence gate (see Gotchas).
- `apply_scoped_sidecar_merge` cannot tell a changes-only run from a selected-files run without changing selected-files behaviour.
