# 02 — Arena round 1 gets the general review's rules

Status: ready-for-agent
Idea: 6 in the original idea list (summarised in ../README.md)
Depends on: —
Impact: M · Complexity: L · Size: S
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — The general review tells its agent three things the arena's first round never hears: what not to flag (naming, style, formatting), how many findings it may raise, and that each finding must point at code it actually read. Arena reviewers get one line: "Propose real issues grounded in the diff."
- **Why it matters** — The arena is the most expensive review er runs, and it is the one picked when a single pass is not trusted. Without those rules its first round produces more style remarks and unbounded lists, and the later rounds spend their votes throwing them out.
- **The fix** — Move the do-not-flag list and the caps line out of the general review's prompt into shared pieces, and use the same pieces in the arena's round-1 prompt, plus an evidence rule worded for the arena's output.
- **TL;DR** — Arena round 1 follows the same finding rules as the general review, from one shared copy of the text.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> Findings that are only about style, diagnostics, or cleanup get dropped.

> [Good REVIEW.md files] cap the nits, list paths to skip, and ask for a file and line as evidence instead of a guess from a name.

## Mission & definition of done
`build_arena_round1_prompt_agent` includes the general review's do-not-flag list and gate line, a caps line chosen by lens, and an evidence rule that asks for `path:line` in `body`. The text comes from the same constant and function the general review's preamble uses, so a later edit changes both. The general review's preamble output is unchanged word for word. The arena's round-1 JSON schema and its parser are unchanged.

- [ ] `prompts.rs` has one constant holding the do-not-flag list and gate line, and one function rendering the caps line; `review_rules_preamble_with_hash` uses both.
- [ ] The existing test `review_rules_preamble_expert_caps_stricter` passes unchanged, and a new test pins the gate text word for word.
- [ ] Arena round 1 for `None`/`general` shows `Max 4 findings per file, max 15 total`; for `expert:<id>` it shows `Max 2 findings per file, max 10 total`; for `professor` it shows `Max 3 findings per file, max 12 total` and no do-not-flag list.
- [ ] Arena round 1 still contains its one-line JSON schema and does not contain `sha256sum`, `diff-annotated`, `review.json` or `outside_diff`.
- [ ] `validate_round1_output` accepts a payload shaped like the prompt's example (new guard test in `arena/schema.rs`).
- [ ] `just clippy` and `just comments-check` exit 0.

**Risk:** ordinary. Prompt text only. A mistake shows up as arena reviewers failing schema validation (each failure is logged per reviewer in round 1), which the guard test in Step 4 covers.

## Before you start (orientation)
- **Branch:** feature. Use the current release branch; see ../README.md "Branching".
- **Commands:**
  - Engine tests (prompts): `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine ai::prompts::tests::`
  - Engine tests (arena schema): `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine arena::schema::tests::`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just comments-check`
- **The map**
  - `crates/er-engine/src/ai/prompts.rs`
    - `review_rules_preamble` (:72) and `review_rules_preamble_with_hash` (:84). The format string runs :128-169. The lines to extract are :152-153 (do-not-flag and gate) and :165 (caps line). The named args `per_file = caps.per_file, total = caps.total` are at :171-172.
    - `professor_rules_preamble` (:838) builds its caps as a struct literal at :844-848 (`per_file: 3, total: 12, is_expert: true`).
    - `build_arena_round1_prompt` (:1818) and `build_arena_round1_prompt_agent` (:1823-1845). `agent_lens_block` (:1847-1861).
    - Test module starts at :1922. The preamble tests are at :2348-2407; copy their shape.
  - `crates/er-engine/src/ai/experts.rs:8-31` — `FindingCaps { per_file, total, is_expert }` with `general()` (4/15) and `expert()` (2/10).
  - `crates/er-engine/src/arena/schema.rs` — `Round1Finding` (:7-18), `validate_round1_output` (:67), `parse_severity` (:112). Tests at :135-153.
  - `crates/er-engine/src/arena/merge.rs:74` — `findings_from_round1` (sets `evidence: vec![]`, reads `f.line`, `f.confidence.unwrap_or(0.5)`).
  - `crates/er-engine/src/arena/orchestrator.rs:840` — the only caller of `build_arena_round1_prompt_agent`.
- **Conventions to follow** — the preamble test shape (prompts.rs:2401-2407):

  ```rust
  #[test]
  fn review_rules_preamble_expert_caps_stricter() {
      let expert = review_rules_preamble("/tmp/managed", true, FindingCaps::expert(), None);
      let general = review_rules_preamble("/tmp/managed", true, FindingCaps::general(), None);
      assert!(expert.contains("Max 2 findings per file, max 10 total"));
      assert!(general.contains("Max 4 findings per file, max 15 total"));
  }
  ```

  The current round-1 prompt body (prompts.rs:1830-1844), which you extend:

  ```rust
  r#"You are reviewer "{reviewer_label}" in a multi-agent code review arena.
  {lens}
  Read the pinned diff at `{path}` (do not run git diff).

  Respond with ONLY a single JSON object on stdout (no markdown fences), matching:
  {{"findings":[{{"file":"path","line":12,"title":"short","body":"detail","severity":"high|med|low","confidence":0.0,"tags":[]}}]}}

  Rules:
  - Propose real issues grounded in the diff.
  - severity: high | med | low
  - If no issues: {{"findings":[]}}
  "#,
  ```
- **Mental model** — Every review prompt is a compiled-in Rust string (ADR 0023). The general, expert, triage and professor prompts share `review_rules_preamble_with_hash`, which also carries diff-hash and annotation steps and a word-valued `confidence`. The arena's round 1 is different: the reviewer reads a raw `diff.patch` (no `[h<N> L<M>]` tags, saved by `save_diff_patch`), prints one JSON object on stdout, and the orchestrator parses it with `validate_round1_output`. `Round1Finding.confidence` is `Option<f32>` and `line` is `Option<usize>` with no lenient parser, so one word-valued confidence or one negative line fails that reviewer's whole output, and enough failures drop the run below quorum (ADR 0024). That is why the arena takes only the pieces that carry over, never the whole preamble.
- **ADRs to read first**
  - 0023 — prompts are compiled in; tests assert on them in the same module.
  - 0024 — arena quorum; a failed reviewer counts against it.

## Scope boundaries
- **In scope:** extracting the do-not-flag/gate text and the caps line into shared items in `prompts.rs`; a `FindingCaps::professor()` constructor; the arena round-1 prompt text; tests in `prompts.rs` and `arena/schema.rs`.
- **OUT of scope / DO NOT TOUCH:**
  - The arena JSON schema (`Round1Finding`), `merge.rs`, and the orchestrator.
  - Annotating `diff.patch` with line tags.
  - The severity labels in the arena (`high|med|low` stays as it is).
  - Rounds 2 and 3 (`build_arena_round2_prompt`, `build_arena_round3_prompt`).
  - The text of `expert_lens_instructions`, even though it tells arena reviewers to write "expert JSON" with a `summary` (an existing mismatch; see Gotchas).
  - The "Scope" section of the preamble (filesystem and `git clone` limits).

## Steps
### Step 1 — Pin the current gate text with a test
- **Goal:** a test that fails if the extraction in Step 2 changes the general preamble's wording.
- **Why / invariant:** the general, expert, triage and professor prompts must read exactly as before. This is a guard test (it passes now); Step 2 must keep it green.
- **Files:** `crates/er-engine/src/ai/prompts.rs`, test module, next to `review_rules_preamble_expert_caps_stricter` (:2401).
- **Do this:** add

  ```rust
  #[test]
  fn review_rules_preamble_gate_text_unchanged() {
      let general = review_rules_preamble("/tmp/managed", true, FindingCaps::general(), None);
      assert!(general.contains(
          "**Do not flag:** naming, formatting, style, import order, file moves without logic change, comment nits.\n**Gate:** \"Does this affect correctness, security, or reliability?\" — if no, skip."
      ));
      assert!(general.contains("### Finding caps\n- Max 4 findings per file, max 15 total\n- Categories:"));
  }
  ```
- **Watch out for:** the em dash in the gate line is U+2014. Copy it from prompts.rs:153, do not retype it.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine review_rules_preamble` → `test result: ok`, all `review_rules_preamble_*` tests pass.
- **If it fails:** the literal does not match the file. Fix the test literal from the file text; do not edit the preamble in this step.

### Step 2 — Extract the shared pieces (no output change)
- **Goal:** one constant and one function, used by the preamble, ready for the arena.
- **Why / invariant:** one copy of the text. The preamble's output is unchanged byte for byte.
- **Files:** `crates/er-engine/src/ai/prompts.rs:84-173`; `crates/er-engine/src/ai/experts.rs:16-31`.
- **Do this:**
  1. Above `review_rules_preamble` (prompts.rs:68), add:

     ```rust
     /// What a finder must not raise, and the question every finding has to pass.
     /// Shared by the general preamble and the arena's first round so the two
     /// cannot drift apart.
     const FINDING_GATE_RULES: &str = "**Do not flag:** naming, formatting, style, import order, file moves without logic change, comment nits.\n**Gate:** \"Does this affect correctness, security, or reliability?\" — if no, skip.";

     fn finding_caps_line(caps: FindingCaps) -> String {
         format!("Max {} findings per file, max {} total", caps.per_file, caps.total)
     }
     ```
  2. In the preamble's format string, replace lines :152-153 with `{gate_rules}` and line :165 `- Max {per_file} findings per file, max {total} total` with `- {caps_line}`.
  3. Replace the named args at :171-172 with `gate_rules = FINDING_GATE_RULES, caps_line = finding_caps_line(caps),`. Leaving `per_file`/`total` in place is a compile error ("named argument never used").
  4. In `experts.rs`, add beside `expert()`:

     ```rust
     pub const fn professor() -> Self {
         Self {
             per_file: 3,
             total: 12,
             is_expert: true,
         }
     }
     ```
     and change the struct literal at prompts.rs:844-848 to `let caps = FindingCaps::professor();`.
- **Watch out for:** `format!` does not re-parse braces inside an argument's value, so the constant needs no `{{ }}` escaping. Do not touch the triage literals at :1226 and :1261 (`per_file: 0, total: 2`).
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine ai::prompts::tests::` → `test result: ok`, 0 failed. `review_rules_preamble_gate_text_unchanged` and `review_rules_preamble_expert_caps_stricter` both pass.
- **If it fails:** a whitespace or newline difference. Compare the constant with the old lines 152-153; the two lines are joined by a single `\n`.

### Step 3 — Write the arena tests first
- **Goal:** tests that describe the new round-1 prompt and fail now.
- **Why / invariant:** the arena keeps its own schema and gets the shared rules.
- **Files:** `crates/er-engine/src/ai/prompts.rs` test module (add an `// ── arena round 1 ──` divider after the preamble tests).
- **Do this:** add three tests:

  ```rust
  #[test]
  fn arena_round1_prompt_carries_general_review_rules() {
      for kind in [None, Some("general")] {
          let p = build_arena_round1_prompt_agent("/tmp/run/diff.patch", "r1", kind);
          assert!(p.contains(FINDING_GATE_RULES), "{kind:?}");
          assert!(p.contains("Max 4 findings per file, max 15 total"), "{kind:?}");
          assert!(p.contains("`path:line`"), "{kind:?}");
      }
  }

  #[test]
  fn arena_round1_caps_follow_the_lens() {
      let expert = build_arena_round1_prompt_agent("/tmp/d.patch", "r1", Some("expert:security"));
      assert!(expert.contains("Max 2 findings per file, max 10 total"));
      assert!(expert.contains(FINDING_GATE_RULES));
      let professor = build_arena_round1_prompt_agent("/tmp/d.patch", "r1", Some("professor"));
      assert!(professor.contains("Max 3 findings per file, max 12 total"));
      assert!(!professor.contains("**Do not flag:**"));
  }

  #[test]
  fn arena_round1_prompt_keeps_its_stdout_schema() {
      let p = build_arena_round1_prompt_agent("/tmp/d.patch", "r1", None);
      assert!(p.contains(r#"{"findings":[{"file":"path","line":12,"title":"short","body":"detail","severity":"high|med|low","confidence":0.0,"tags":[]}]}"#));
      assert!(p.contains("confidence: a number from 0.0 to 1.0"));
      for leaked in ["sha256sum", "diff-annotated", "review.json", "outside_diff", "`confirmed`"] {
          assert!(!p.contains(leaked), "arena prompt leaked {leaked}");
      }
  }
  ```
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine arena_round1` → the first two fail (no gate text, no caps line). The third fails only on `confidence: a number from 0.0 to 1.0`. Record the failure output for your report.
- **If it fails differently:** if the schema assert fails, the escaped `{{`/`}}` in the source renders as single braces; the raw string in the test must use single braces.

### Step 4 — Add a schema guard test
- **Goal:** prove the prompt's example payload still parses.
- **Why / invariant:** a reviewer that follows the example must never fail `validate_round1_output`.
- **Files:** `crates/er-engine/src/arena/schema.rs`, tests module (:135), next to `accepts_med_severity_alias`.
- **Do this:**

  ```rust
  #[test]
  fn accepts_round1_payload_shaped_like_the_prompt_example() {
      let v = json!({ "findings": [
          { "file": "path", "line": 12, "title": "short", "body": "detail (src/a.rs:40)", "severity": "med", "confidence": 0.0, "tags": [] },
          { "file": "path", "title": "no line", "body": "detail", "severity": "low" }
      ]});
      assert!(validate_round1_output(&v).is_ok());
  }
  ```
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine arena::schema::tests::` → `test result: ok` (3 passed). This is a guard; it passes before and after.

### Step 5 — Rewrite the arena round-1 prompt
- **Goal:** the tests from Step 3 pass.
- **Why / invariant:** shared text comes from `FINDING_GATE_RULES` and `finding_caps_line`; the arena's own field rules are stated in the arena prompt because its fields differ from the general review's.
- **Files:** `crates/er-engine/src/ai/prompts.rs:1822-1845`.
- **Do this:**
  1. Add a helper below `agent_lens_block`:

     ```rust
     /// Caps for an arena reviewer, matching what the same lens gets outside the arena.
     fn arena_caps(agent_kind: Option<&str>) -> FindingCaps {
         match agent_kind {
             Some("professor") => FindingCaps::professor(),
             Some(k) if k.starts_with("expert:") => FindingCaps::expert(),
             _ => FindingCaps::general(),
         }
     }
     ```
  2. In `build_arena_round1_prompt_agent`, compute

     ```rust
     let caps_line = finding_caps_line(arena_caps(agent_kind));
     // The professor lens writes teaching notes and reports no defects, so the gate
     // would drop everything it is asked to write.
     let gate_rules = if agent_kind == Some("professor") { "" } else { FINDING_GATE_RULES };
     ```
  3. Replace the `Rules:` block with:

     ```text
     Rules:
     - Propose real issues grounded in the diff.
     - severity: high | med | low
     - confidence: a number from 0.0 to 1.0 (not a word)
     - line: the new-side line number of the line you mean; omit `line` for a deleted line, never write a negative number
     - Evidence: in `body`, name the `path:line` of the code you read that shows the problem. Drop a finding you cannot ground in code you read.
     - {caps_line}
     {gate_rules}
     - If no issues: {{"findings":[]}}
     ```
     Keep the schema line exactly as it is.
- **Watch out for:** `{{` and `}}` are literal braces inside this `format!`. The new placeholders are single-brace `{caps_line}` and `{gate_rules}`. When `gate_rules` is empty an empty line remains; that is fine.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine ai::prompts::tests::` → `test result: ok`, 0 failed, including the three `arena_round1_*` tests.
- **Mutation check:** prove `arena_round1_prompt_carries_general_review_rules` catches a missing gate. In one command, back up, plant, run, restore:

  ```bash
  cp crates/er-engine/src/ai/prompts.rs "$TMPDIR/prompts.rs.bak" && sed -i '' 's/else { FINDING_GATE_RULES }/else { "" }/' crates/er-engine/src/ai/prompts.rs; ~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine arena_round1_prompt_carries; cp "$TMPDIR/prompts.rs.bak" crates/er-engine/src/ai/prompts.rs
  ```
  The `sed` targets only the arena's `let gate_rules = … else { FINDING_GATE_RULES };` line, so the preamble still compiles. Expect a test failure (not a compile error), then `git diff --stat` to show only your intended changes. If a hook blocks the chained command, run the three parts as separate calls back to back and restore right after the test run.
- **If it fails:** a test expecting `FINDING_GATE_RULES` on the professor lens means `gate_rules` was not cleared for `professor`.

## Gotchas & landmines
- `Round1Finding.line` is `Option<usize>` with no lenient deserializer (schema.rs:9-10), unlike `Finding.line_start` in review.rs. A model copying a negative tag fails the whole reviewer. That is why the new `line:` rule exists.
- `agent_lens_block` for `expert:<id>` returns `expert_lens_instructions`, which says "Write findings and a lens-specific `summary` in the expert JSON — do not write order.json, checklist.json, or summary.md." That does not match the arena's stdout output. It predates this task; leave it, and do not copy that text anywhere new.
- `build_arena_round1_prompt` (:1818) has no caller outside `prompts.rs`. Leave it; it delegates to the `_agent` variant and picks up the change.
- The preamble is also used by triage (caps 0/2) and professor. Step 2 changes none of their output, and Step 1's test is what proves it for the general case.
- `just comments-check` rejects comments that cite a plan step or task number. The doc comments in Steps 2 and 5 say why, never "task 02".

## Decision log (forks pre-answered)
- **If you wonder whether to reuse the whole `review_rules_preamble`:** do not. It carries hash steps, `[h<N> L<M>]` tag rules, a word-valued confidence and the `outside_diff` field, all of which break or confuse the arena's stdout schema.
- **Severity meanings (P0/P1/P2) are not shared.** The general block uses `medium`/`info`, the arena schema says `med` and has no `info`. Changing arena labels is out of scope; the arena keeps `severity: high | med | low`.
- **Evidence is reworded for the arena.** Round 1 has no `evidence` field (`findings_from_round1` sets `evidence: vec![]`), so the rule asks for `path:line` inside `body`. Adding an `evidence` field to the arena schema is out of scope.
- **Professor lens:** caps 3/12 (same as `professor_rules_preamble`), no gate rules, because the gate asks "does this affect correctness" and the professor lens writes teaching notes.
- **`diff.patch` stays unannotated.** Tagging it would let arena reviewers copy line numbers, but it changes the arena's inputs and its arbiter excerpt; not part of this task.
- **Unknown `agent_kind` (anything else):** general caps and the gate, same as `None`.

## Full verification
1. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine ai::prompts::tests::` → `test result: ok`, 0 failed.
2. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine arena::` → `test result: ok`, 0 failed.
3. `~/.claude/scripts/locked.sh . just clippy` → exit 0.
4. `~/.claude/scripts/locked.sh . just comments-check` → exit 0.
5. `git diff --stat` → only `crates/er-engine/src/ai/prompts.rs`, `crates/er-engine/src/ai/experts.rs`, `crates/er-engine/src/arena/schema.rs`.

## Rollback
Revert the commit. No stored data changes shape; arena runs started with the new prompt keep their saved round outputs, which parse under the unchanged schema.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- Step 2 changes the output of any existing prompt test (a test outside the ones added here starts failing).
- You find a second caller of `build_arena_round1_prompt_agent` that passes an `agent_kind` not covered by `arena_caps`.

## Comments
