# 06 — Pre-existing tag

Status: needs-design
Idea: 5 in the original idea list (summarised in ../README.md)
Depends on: — (an ADR must be approved first, Step 1)
Impact: M · Complexity: M · Size: M
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — er's reviewers may only point at lines the change added or removed. A real bug sitting two lines away in unchanged code either goes unreported, or gets pinned to a changed line and reads as the author's mistake. When the model does admit a bug is old, it grades it `informational`, and once an arbiter has graded the review that grade is hidden.
- **Why it matters** — The owner decided Pre-existing findings are wanted (README, "Decisions already made"). A bug the PR did not introduce is still a bug, and the reviewer should see it without it being blamed on the author.
- **The fix** — Let a finding sit on an unchanged line that the diff already shows (a context line inside a hunk), with a new `pre_existing` flag that is separate from confidence. Show those findings in their own group in the TUI and the desktop, labelled Pre-existing, and cap how many a review may raise.
- **TL;DR** — Reviewers can report old bugs they see in the diff's surrounding lines, marked Pre-existing and shown apart, so they never read as the author's fault.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> What's left shows up as inline comments sorted by severity. Each is tagged Important, Nit, or Pre-existing, so a bug the PR didn't introduce doesn't look like the author's fault.

From the Code Review docs (https://code.claude.com/docs/en/code-review): a later review that starts on its own "posts fewer nits and pre-existing findings". That later-review behaviour is task 08; this task only adds the flag and its cap.

## Mission & definition of done
After an approved ADR, `Finding` carries `pre_existing: bool`. The general and expert review prompts allow a pre-existing finding on a tagged context line, with `outside_diff: true` and `pre_existing: true`, capped (3 for the general review, 1 per expert lens). A bug this change caused in unchanged code still anchors on the `+`/`-` line that caused it. Triage and professor prompts keep the strict rule word for word. Confidence no longer means "pre-existing", so the gate treats these findings like any other. The TUI panel, the TUI inline banner, the desktop AI Review card and the desktop inline finding show Pre-existing findings apart and labelled. Exports and the MCP schema carry the flag.

- [ ] ADR `docs/adr/<next>-pre-existing-findings-on-context-lines.md` exists and the owner approved it (Step 1). `CONTEXT.md` defines **Pre-existing**.
- [ ] `Finding.pre_existing` exists with `#[serde(default)]`; an old sidecar without it loads as `false` (test).
- [ ] `FindingCaps` has a `pre_existing` cap; the preamble text with cap 0 is unchanged word for word (test), and with cap > 0 carries the new rules (test).
- [ ] The existing test `review_prompt_requires_findings_to_anchor_to_plus_or_minus` passes unchanged.
- [ ] TUI panel header counts Pre-existing findings separately and draws them in their own group; the inline banner labels them (tests on the extracted pure helpers).
- [ ] Desktop `FlatFinding.pre_existing` exists; `splitPreExisting` in `aiReviewAgents.ts` has tests; the AI Review card draws a separate "Pre-existing" group; `InlineFinding.svelte` shows a label.
- [ ] `sidecar_specs.rs`, `pr_review_feedback.rs`, `export.rs` and `skills/REVIEW_RULES.md` carry the flag.
- [ ] `just clippy`, `just lint-ui`, `just docs-check`, `just comments-check` exit 0.

**Risk:** ordinary. Sidecar change is additive (`#[serde(default)]`). The main risk is the model filing regressions as Pre-existing; the prompt's explicit regression rule and the cap limit that.

## Before you start (orientation)
- **Branch:** feature. Use the current release branch; see ../README.md "Branching".
- **Commands:**
  - Engine tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine <filter>`
  - TUI tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui <filter>`
  - Desktop backend: `~/.claude/scripts/locked.sh . cargo test -p er-desktop <filter>`
  - Frontend: `~/.claude/scripts/locked.sh desktop-ui bun run test`, `~/.claude/scripts/locked.sh desktop-ui bun run check`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`, `~/.claude/scripts/locked.sh . just docs-check`, `~/.claude/scripts/locked.sh . just comments-check`
- **The map**
  - `docs/adr/` — new ADR (next free number; 0043 is the highest at this commit; task 05 may take 0044). Format: `.agents/skills/domain-modeling/ADR-FORMAT.md`.
  - `CONTEXT.md` — new term **Pre-existing**.
  - `crates/er-engine/src/ai/review.rs`
    - `min_trust_for` (:35) — the gate default (Tentative once an arbiter has graded, else Informational).
    - `Confidence` (:198-211); the `Informational` doc at :207 says "Real, but pre-existing or low-impact relative to this PR."
    - `pub struct Finding` (:271); `outside_diff` (:313-318) with its doc; `confidence` (:322).
    - `Finding::passes` (:364), `refresh_stale` (:382-403), `lens_category_tag` (:440).
    - Tests: `refresh_stale_is_per_finding` (:1819, already anchors on `Context` lines), `make_finding` (:2130), `make_finding_with_lines` (:2158), `findings_for_line_matches_by_hunk_and_line` (:2575).
  - `crates/er-engine/src/ai/experts.rs:8-31` — `FindingCaps`.
  - `crates/er-engine/src/ai/prompts.rs`
    - Preamble `review_rules_preamble_with_hash` (:84-173). Anchor block :140-144. Caps :164-166.
    - `general_review_instructions_read_analyze` (:176-183); its first line says "**Findings target only `+` or `-` lines** — context is for comprehension." It is also used by triage (:1245, :1281) and professor (:1060, :1093).
    - `general_review_json_example` (:186-213, `"outside_diff": false` at :198), used by the general (:225) and expert (:371) output sections.
    - Professor JSON example `"outside_diff": false` (:922) — leave it.
    - `FindingCaps` struct literals: professor (:844-848), triage (:1226-1230, :1261-1265).
    - `build_validate_prompt_prepared_diff` (:1412).
    - Test `review_prompt_requires_findings_to_anchor_to_plus_or_minus` (:2167); preamble tests (:2348-2407).
  - `crates/er-engine/src/ai/prepared_diff.rs:152-182` — `annotate_diff_raw` tags context lines `[h<N> L<M>]` (:173-176), same as `+` lines.
  - `crates/er-engine/src/ai/relocate.rs:27-49` — `relocate_comment`; passes 1 and 2 skip only `LineType::Delete`, so context lines relocate.
  - `crates/er-engine/src/git/diff.rs:428` — the parser folds context runs longer than `2 * DEFAULT_CONTEXT_LINES + 1` (= 21) lines; `DEFAULT_CONTEXT_LINES` is 10 (`crates/er-engine/src/git/mod.rs:9`).
  - `crates/er-engine/src/pr_review_feedback.rs` — parallel struct with `outside_diff` (:34) and its mapping (:168).
  - `crates/er-engine/src/sidecar_specs.rs` — review schema `"outside_diff"` (:138) and example (:388).
  - `crates/er-engine/src/export.rs:466-470` — the `[outside diff]` badge.
  - `skills/REVIEW_RULES.md:18-20` — provenance doc for the anchor rules (ADR 0023 says the binary does not read it).
  - TUI: `crates/er-tui/src/ui/panel.rs` — `render_file_findings` (:410, sort :430-445, gate filter :450-454, header :463-469), `push_finding` (:546); tests module (:1473). `crates/er-tui/src/ui/diff_view.rs` — `render_finding_banner` (:2735, tag span :2767-2773).
  - Desktop: `crates/er-desktop/src/snapshot.rs` — `FlatFinding` (:1146-1181), `to_flat` (:4217-4295). `desktop-ui/src/lib/types.ts:178-205` — `FlatFinding`. `desktop-ui/src/lib/aiReviewAgents.ts` (helpers, tested in `aiReviewAgents.test.ts`). `desktop-ui/src/lib/components/AiReviewCard.svelte` — `ordered` (:149-160), list (:484-545). `desktop-ui/src/lib/components/InlineFinding.svelte` (lens pill near :35). `desktop-ui/src/lib/stories/fixtures.ts` (FlatFinding fixtures at :451, :468, :485).
- **Conventions to follow**
  - The preamble's anchor block today (prompts.rs:140-144), which must stay word for word when the Pre-existing cap is 0:

    ```text
    ### Annotate and anchor
    - Findings **only** on `+` or `-` lines in the annotated diff
    - Copy `hunk_index` from `[h<N>]` and `line_start` from `[h<N> L<M>]` — **never** count or infer line numbers
    - Deleted lines are tagged `[h<N> L-<M>]` (note the minus). For a finding on a deleted line, set `line_start: null` and anchor by `hunk_index` alone — **never** write a negative `line_start`
    - Set `outside_diff: false`; drop findings you cannot anchor to a tagged line
    ```
  - Adding an additive sidecar field, as `Finding.raised_by` did (review.rs):

    ```rust
    /// Every lens that raised this claim. ... Empty on sidecars written before it existed, and
    /// treated as `[lens]` when read.
    #[serde(default)]
    pub raised_by: Vec<String>,
    ```
  - A TUI pure-helper test, `reach_line` (panel.rs:1446) and its tests (:1476-1500).
- **Mental model** — A review agent reads an annotated diff where every content line carries `[h<N> L<M>]`, including the context lines around each change (10 lines each side at er's default). It writes `review.json`; er loads it, overlays expert findings and arbiter verdicts, and draws each finding at `(file, line_start)`. Drawing (`findings_for_line`, review.rs:1038) and staleness (`refresh_stale` → `relocate_comment`) both work on any non-deleted line in a hunk, so a finding on a context line already behaves like one on a `+` line. A line outside every hunk is different: it has no row to draw on, and `refresh_stale` would call `relocate_comment`, get `Lost`, and mark it stale at once (ADR 0041 solved that only for questions and notes). The gate (`Finding::passes`) compares confidence only. Today a pre-existing bug gets `informational`, which is what hides it after an arbiter pass.
- **ADRs to read first**
  - 0009 — findings are AI-owned; this task adds a field the AI sets, er never edits it.
  - 0010 — staleness is two conditions; a finding on a context line uses the existing line-content check.
  - 0035 — arbiter verdicts overlay findings by `finding_key = sha1(file + line + canonical(title))`; unaffected by the new field.
  - 0041 — document anchors exist for questions and notes only; this task does not extend them to findings.
  - 0006 — engine state is the UI contract; the new flag is a new field, never a second meaning on `outside_diff`.

## Scope boundaries
- **In scope:** the ADR and `CONTEXT.md` entry; `Finding.pre_existing`; `FindingCaps.pre_existing`; preamble, JSON example, analyze line and validate prompt text; TUI panel and inline banner; desktop snapshot, types, AI Review card and inline finding; MCP schema, PR feedback struct, export badge, `skills/REVIEW_RULES.md`.
- **OUT of scope / DO NOT TOUCH:**
  - The gate logic in `Finding::passes` and `min_trust_for`.
  - Findings on lines outside every hunk, and any change to ADR 0041 anchors.
  - Arena and seeded runs: `arena/import.rs:70` hardcodes `outside_diff: false` and the arena schema has no such field; seeded dedupe and import drop the flag. Leave them. Task 07 Step 4 carries `pre_existing` through the seeded run and the arbiter prompt when it lands; arena imports (07's Deep depth) still drop it.
  - Important/Nit labels in either front end. The mapping is recorded in the Decision log. No task in this package draws Important/Nit labels yet: 07 ranks by arbiter verdicts and severity only, and 08 and 09 use the High/Medium side of the mapping as "blocking" and "must fix".
  - Posting fewer Pre-existing findings on later runs, and excluding them from a round cap (task 08).
  - Triage and professor prompt text.

## Steps
### Step 1 — Write the ADR and the CONTEXT entry, then stop for approval
- **Goal:** the owner approves relaxing the `+`/`-` rule before code changes.
- **Why / invariant:** the rule was deliberate in the prompt; the README says this task needs an ADR.
- **Files:** `docs/adr/<next>-pre-existing-findings-on-context-lines.md`; `CONTEXT.md`.
- **Do this:**
  1. Research the old rule: `git log --format='%h %ad %s' --date=short -S 'Findings **only** on' -- crates/er-engine/src/ai/prompts.rs skills/REVIEW_RULES.md`. At c96dc0f12 it returns only `dfb706d71 2026-05-24 Add specialized expert reviewers and polish desktop review UX.`, which gives no reason. Read `git show dfb706d71 --stat` and its message; if no reason appears, the ADR says no recorded reason was found. Do not invent one.
  2. Write the ADR (house style, short; tone of `docs/adr/0041-document-anchored-questions-and-notes.md`). It states:
     - **Decision.** A finding may anchor on a context line inside a hunk (a line the annotated diff tags `[h<N> L<M>]` with no `+`/`-`) only when it reports a bug the change did not introduce. Such a finding sets `pre_existing: true` and `outside_diff: true`. A bug the change causes in unchanged code still anchors on the `+`/`-` line that caused it. Lines outside every hunk stay out of reach.
     - **Why a new field.** `outside_diff` says where the anchor sits; `pre_existing` says who introduced the bug. A review of code outside any diff (task 18) has findings that are pre-existing with no diff to sit outside of, and ADR 0006 forbids giving an existing field a second meaning.
     - **Why separate from Confidence.** Confidence is how far a grade can be trusted (CONTEXT.md). Folding "old" into `informational` is what made the gate hide real bugs once an arbiter graded the review. The gate stays a confidence policy.
     - **Why context lines only.** They already draw, relocate and go stale like `+` lines. Lines outside every hunk have no row and would be marked stale at once.
     - **Cap.** 3 per general review, 1 per expert lens, inside the existing caps.
     - **Consequences.** A context run longer than 21 lines between two changes is folded by the parser, and a finding on a folded line would read as stale. Promoting a Pre-existing finding to a GitHub comment depends on GitHub accepting a comment on that context line (resolve the "Promote to GitHub" TODO in Gotchas while writing this). Seeded and arena runs do not carry `pre_existing` until task 07 lands; until then an arbiter can grade a Pre-existing expert finding down to `informational` without knowing it is old, and the gate may then hide it. Task 07 Step 4 carries the flag through the seeded run and tells the arbiter not to grade a finding down for being old. Arena imports (Deep) still set it to `false`.
  3. Add to `CONTEXT.md` (in **Language**, after **Confidence**):

     ```markdown
     **Pre-existing**:
     Said of a finding about a bug the change under review did not introduce. It
     sits on a line the diff shows but did not change, and it is shown apart from
     the change's own findings so it never reads as the author's fault. Separate
     from Confidence: one says who introduced the bug, the other how far the grade
     can be trusted.
     _Avoid_: legacy, old bug, outside diff
     ```
- **Verify:** `~/.claude/scripts/locked.sh . just docs-check` → exit 0.
- **STOP here.** Report the ADR path and the caps, and ask the owner to approve. Do not start Step 2 without a yes.

### Step 2 — Add the field, test first
- **Goal:** `Finding.pre_existing` exists, loads as `false` from old sidecars, and `outside_diff`'s doc says only where the anchor sits.
- **Why / invariant:** additive sidecar change; every existing `review.json` still loads.
- **Files:** `crates/er-engine/src/ai/review.rs:207`, `:313-318`; every struct literal the compiler reports.
- **Do this:**
  1. Test first, in review.rs tests:

     ```rust
     #[test]
     fn pre_existing_defaults_to_false_on_old_sidecars() {
         let json = r#"{"id":"f-1","severity":"low","title":"t","outside_diff":true}"#;
         let f: Finding = serde_json::from_str(json).unwrap();
         assert!(!f.pre_existing);
         let marked: Finding = serde_json::from_str(
             r#"{"id":"f-2","severity":"low","title":"t","pre_existing":true}"#,
         ).unwrap();
         assert!(marked.pre_existing);
     }
     ```
     It fails to compile until the field exists; that is the failing state.
  2. After `outside_diff` add:

     ```rust
     /// The finding reports a bug the change under review did not introduce.
     /// Set by the producer; drawn apart from the change's own findings so it
     /// never reads as the author's fault. Separate from `confidence`, which says
     /// how far the grade can be trusted and says nothing about who wrote the code.
     #[serde(default)]
     pub pre_existing: bool,
     ```
  3. Rewrite the `outside_diff` doc to: "True when the finding anchors on a line the change did not add or remove (a context line in a hunk). Says where the anchor sits; whether the bug predates the change is `pre_existing`."
  4. Change the `Confidence::Informational` doc (:207) to "Real, but low-impact relative to this change. Optional fix."
  5. Add `pre_existing: false,` to every `Finding { … }` literal the compiler reports. At c96dc0f12 they sit next to `outside_diff: false,` in: `export.rs:990, :1160, :1185`; `ai/experts.rs:451`; `ai/finding_responses.rs:377, :434`; `ai/finding_cleanup.rs:265`; `ai/review.rs:2146, :2180`; `ai/professor.rs:117`; `ai/scoped_merge.rs:314`; `arena/import.rs:70`; `pr_review_feedback.rs:643` (a `Finding` literal in its tests); `crates/er-desktop/src/snapshot.rs:6001`.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pre_existing_defaults` → 1 passed; `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine` → `test result: ok`.
- **If it fails:** a missing literal shows as `missing field pre_existing` with a path; add it there.

### Step 3 — Prompt rules behind a cap
- **Goal:** general and expert prompts allow Pre-existing findings on context lines; triage and professor do not change.
- **Why / invariant:** the cap decides; with cap 0 the anchor block and analyze line read as they do today.
- **Files:** `crates/er-engine/src/ai/experts.rs:8-31`; `crates/er-engine/src/ai/prompts.rs:84-213`, `:844-848`, `:1226-1230`, `:1261-1265`, `:1412-1450`.
- **Do this:**
  1. Tests first, in prompts.rs tests beside the preamble tests:

     ```rust
     #[test]
     fn preamble_without_pre_existing_cap_keeps_the_strict_anchor_rule() {
         let caps = FindingCaps { per_file: 0, total: 2, is_expert: true, pre_existing: 0 };
         let p = review_rules_preamble("/tmp/m", true, caps, None);
         assert!(p.contains("- Findings **only** on `+` or `-` lines in the annotated diff\n"));
         assert!(p.contains("- Set `outside_diff: false`; drop findings you cannot anchor to a tagged line"));
         assert!(!p.contains("pre_existing"));
     }

     #[test]
     fn general_and_expert_prompts_allow_capped_pre_existing_findings() {
         let general = build_review_prompt_prepared_diff("branch", "/tmp/out", "main", "feat", HASH);
         assert!(general.contains("### Pre-existing findings"));
         assert!(general.contains("Max 3 pre-existing findings"));
         assert!(general.contains("\"pre_existing\": false"));
         assert!(general.contains("not pre-existing"));
         let expert = build_expert_review_prompt_prepared_diff("branch", "/tmp/out", "security", HASH);
         assert!(expert.contains("Max 1 pre-existing finding"));
         let triage = build_triage_review_prompt_prepared_diff("branch", "/tmp/out", HASH);
         assert!(!triage.contains("### Pre-existing findings"));
     }
     ```
     Run: they fail (no field, no section).
  2. Add `pub pre_existing: u32` to `FindingCaps`: `general()` → 3, `expert()` → 1. Add `pre_existing: 0` to the literals at prompts.rs:844 (professor; if task 02 landed, add it in `FindingCaps::professor()` instead) and :1226, :1261 (triage).
  3. In the preamble, build the anchor block from `caps.pre_existing`:
     - `0` → today's four lines exactly.
     - `> 0` →

       ```text
       - Findings **only** on `+` or `-` lines in the annotated diff, except a pre-existing finding (below)
       - Copy `hunk_index` from `[h<N>]` and `line_start` from `[h<N> L<M>]` — **never** count or infer line numbers
       - Deleted lines are tagged `[h<N> L-<M>]` (note the minus). For a finding on a deleted line, set `line_start: null` and anchor by `hunk_index` alone — **never** write a negative `line_start`
       - For every other finding set `outside_diff: false` and `pre_existing: false`; drop findings you cannot anchor to a tagged line

       ### Pre-existing findings
       - A real bug in code this change did not write or alter, seen while reviewing. Anchor it on a tagged context line (no `+` or `-`) and set `outside_diff: true` and `pre_existing: true`
       - If this change makes unchanged code fail (a caller now passes the wrong value, a check it relied on was removed), that is not pre-existing: anchor it on the `+` or `-` line that caused it, with both set to `false`
       - Only lines that carry a `[h<N> L<M>]` tag. A bug outside every hunk is out of reach; leave it out
       - Grade `confidence` on whether the bug is real, as for any finding. Never lower it because the code is old
       - Max {n} pre-existing finding(s) in total; they count toward the caps below
       ```
       Render `{n}` as `3 pre-existing findings` / `1 pre-existing finding` so the tests' strings match.
  4. Change the first line of `general_review_instructions_read_analyze` (:177) from "**Findings target only `+` or `-` lines** — context is for comprehension." to "**Anchor findings as *Annotate and anchor* says** — context is for comprehension." It is shared with triage and professor, whose preamble still says `+`/`-` only, so this is not a loosening for them. Grep the test module for `Findings target only` first; at c96dc0f12 no test asserts it.
  5. In `general_review_json_example`, add `"pre_existing": false,` on the line after `"outside_diff": false,` (:198). Leave the professor example (:922).
  6. In `build_validate_prompt_prepared_diff`, add to step 5: "Keep `pre_existing` and `outside_diff` as they are. Do not lower a finding's confidence because it is pre-existing."
- **Watch out for:** the preamble is a `format!` with `{{`/`}}` for literal braces. Build the anchor block as a separate `String` and insert it with a named arg, as task 02 does for the caps line. The existing test `review_prompt_requires_findings_to_anchor_to_plus_or_minus` (:2167) asserts the substring "Findings **only** on `+` or `-` lines"; the new first line keeps that substring, so the test passes unchanged. Do not edit that test.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine ai::prompts::tests::` → `test result: ok`, 0 failed.
- **Mutation check:** set `general()`'s `pre_existing` to 0, run `general_and_expert_prompts_allow_capped_pre_existing_findings`, see it fail, restore (back up and restore `experts.rs` in the same command: `cp crates/er-engine/src/ai/experts.rs "$TMPDIR/experts.rs.bak" && <edit>; <test>; cp "$TMPDIR/experts.rs.bak" crates/er-engine/src/ai/experts.rs`).

### Step 4 — Carry the flag through schema, feedback and export
- **Goal:** every place that describes or copies a finding knows the flag.
- **Why / invariant:** MCP agents author `review.json` from `sidecar_specs.rs`; PR feedback and exports are read by people and agents.
- **Files:** `crates/er-engine/src/sidecar_specs.rs:138`, `:388`; `crates/er-engine/src/pr_review_feedback.rs:34`, `:168`; `crates/er-engine/src/export.rs:466-470`; `skills/REVIEW_RULES.md:18-20`.
- **Do this:**
  1. Schema: after `"outside_diff": { "type": "boolean" },` add `"pre_existing": { "type": "boolean", "description": "Bug the change did not introduce; anchor on a context line with outside_diff true" },`. Example (:388): add `"pre_existing": false,`.
  2. `pr_review_feedback.rs`: add `pub pre_existing: bool` after `outside_diff` (:34), and map `pre_existing: f.pre_existing` (:168). (The literal at :643 is a `Finding`, already handled in Step 2.)
  3. `export.rs:466`: after the `outside` badge, add `let pre_existing = if f.pre_existing { " [pre-existing]" } else { "" };` and include it in the header `format!` before `{outside}`.
  4. `skills/REVIEW_RULES.md`: change item 3 to "Findings only on `+` or `-` lines, except a capped pre-existing finding on a context line" and item 5 to "Set `outside_diff` and `pre_existing` to `false` except on a pre-existing finding; drop unanchored findings".
  5. Tests: no export test asserts `[outside diff]` at c96dc0f12 (the string appears only at export.rs:467). Add `pre_existing_badge_in_export` to export.rs's test module, copying the test that builds a tab with an `ErFileReview` and a `Finding { id: "f-keep", … }` literal (export.rs:1140-1215) and calls `render_markdown(&tab, &opts)`. Set `pre_existing: true` on the finding and assert the output contains `[pre-existing]`. The badge itself is written by `render_item` (:363). Add `review_spec_lists_pre_existing` to `sidecar_specs.rs`'s tests (they start at :549), asserting the review spec JSON contains `"pre_existing"`.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine export` → ok; `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine sidecar_specs` → ok; `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pr_review_feedback` → ok.

### Step 5 — TUI: own group and label
- **Goal:** the file findings panel counts and draws Pre-existing findings apart; the inline banner labels them.
- **Why / invariant:** Pre-existing never mixes into the change's own list. The gate still applies to them (`passes` unchanged).
- **Files:** `crates/er-tui/src/ui/panel.rs:410-475`, tests (:1473); `crates/er-tui/src/ui/diff_view.rs:2735-2780`.
- **Do this:**
  1. Extract the header into a pure function and test it first:

     ```rust
     fn findings_header(visible: usize, pre_existing: usize, below_gate: usize, resolved: usize) -> String
     ```
     Output: ` Findings (V)`, then ` · P pre-existing` when P > 0, then the existing ` · N below gate` and ` · N resolved` parts. `V` counts every visible finding, Pre-existing included (see the counting rule in the Decision log). Tests: `findings_header_counts_pre_existing_apart` (`(3, 1, 0, 0)` → `" Findings (3) · 1 pre-existing"`) and `findings_header_unchanged_without_pre_existing` (`(2, 0, 1, 1)` → `" Findings (2) · 1 below gate · 1 resolved"`).
  2. In `render_file_findings`, split `visible` into `current` (`!f.pre_existing`) and `old` (`f.pre_existing`), keeping the existing sort. Draw `current` as today; then, when `old` is non-empty, a dim line ` Pre-existing (not introduced by this change)` and the `old` rows with `push_finding`.
  3. In `render_finding_banner`, when `finding.pre_existing`, push a span `[pre-existing] ` styled `styles::PURPLE()` before the lens/category span (:2767). Do the same at the start of the title in `push_finding` (panel.rs:598).
- **Watch out for:** colors go through `styles.rs` accessors only (`crates/er-tui/src/ui/CLAUDE.md`). Keep the `below_gate` arithmetic (:459-461) correct: `below_gate = total − visible(all) − resolved − dropped`, computed before the split.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui findings_header` → 2 passed; `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui` → ok.

### Step 6 — Desktop: own group and label
- **Goal:** the AI Review card lists Pre-existing findings in their own group; the inline finding carries a label.
- **Why / invariant:** the engine decides; the front end reads `pre_existing` and does not infer it from `confidence` or anything else (ADR 0006).
- **Files:** `crates/er-desktop/src/snapshot.rs:1146-1181`, `:4278-4295`; `desktop-ui/src/lib/types.ts:178-205`; `desktop-ui/src/lib/aiReviewAgents.ts`, `aiReviewAgents.test.ts`; `desktop-ui/src/lib/components/AiReviewCard.svelte:149-160`, `:484-545`; `desktop-ui/src/lib/components/InlineFinding.svelte`; `desktop-ui/src/lib/stories/fixtures.ts`.
- **Do this:**
  1. `FlatFinding` (Rust): add `/// The finding reports a bug the change did not introduce.` `#[serde(default)] pub pre_existing: bool,` and set `pre_existing: f.pre_existing` in `to_flat`. Add a desktop test `flat_finding_carries_pre_existing` beside `flat_finding_carries_grade_and_lens_category` (snapshot.rs:6123), using its `ai_snapshot_with(vec![test_finding(…)])` helpers with `pre_existing` set to `true` on the finding, and assert `snapshot.findings[0].pre_existing`.
  2. `types.ts`: add `/** Bug the change did not introduce; drawn apart. */ pre_existing: boolean;` to `FlatFinding`. Add `pre_existing: false` to each fixture in `stories/fixtures.ts` (`bun run check` lists any other literal).
  3. `aiReviewAgents.ts`: add

     ```ts
     /** The change's own findings, and the pre-existing ones drawn in their own group. */
     export function splitPreExisting(findings: FlatFinding[]): { current: FlatFinding[]; preExisting: FlatFinding[] }
     ```
     preserving order. Tests in `aiReviewAgents.test.ts`: order kept within each group; empty input gives two empty arrays.
  4. `AiReviewCard.svelte`: derive `const groups = $derived(splitPreExisting(ordered));`, render `groups.current` in the existing `findings-list`, then, when `groups.preExisting.length > 0`, a small heading `Pre-existing ({n}) · not introduced by this change` and the same row markup for `groups.preExisting`. Move the row markup into a `{#snippet}` used by both loops rather than copying it.
  5. `InlineFinding.svelte`: when `finding.pre_existing`, render a pill `Pre-existing` with the same classes the card uses for its confidence pill (`px-1 py-0.5 rounded border border-hairline text-[9px] text-muted`).
- **Watch out for:** `ordered` sorts by severity then trust; keep that order inside each group. Do not change `findingPassesTrust`. If the lint gate flags the component for size or cohesion, split along the groups the warning names (ADR 0038); do not grow a baseline.
- **Verify:** `~/.claude/scripts/locked.sh desktop-ui bun run test` → all pass, including the `splitPreExisting` tests; `~/.claude/scripts/locked.sh desktop-ui bun run check` → 0 errors; `~/.claude/scripts/locked.sh . cargo test -p er-desktop` → ok.
- **Visual check:** per the desktop visual-check routine (Storybook on a spare port), add `pre_existing: true` to one fixture used by the AI Review card story and confirm the separate group renders.

### Step 7 — Guard test for context-line anchors
- **Goal:** prove a Pre-existing finding on a context line stays fresh, and that the test would catch a change to that.
- **Why / invariant:** context-line anchors rely on relocation scanning context lines; nothing else pins that for findings.
- **Files:** `crates/er-engine/src/ai/review.rs` tests, beside `refresh_stale_is_per_finding` (:1819).
- **Do this:** add `pre_existing_finding_on_a_context_line_stays_fresh`: build the diff file the same way as `refresh_stale_is_per_finding`, a finding with `pre_existing: true`, `outside_diff: true`, `line_start: Some(3)`, `line_content: "    let kept = 1;"`, call `refresh_stale(Some(&file))`, assert `!stale`. Then put the same finding in an `AiState` and assert `state.findings_for_line("a.rs", 0, 3, &layers())` returns it, copying `findings_for_line_matches_by_hunk_and_line` (:2575-2589; the method is `AiState::findings_for_line(&self, path, hunk_index, line_num, layers) -> Vec<&Finding>`, :1038).
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pre_existing_finding_on_a_context_line` → 1 passed. This passes before any code change in this task; it is a guard.
- **Mutation check:** `relocate.rs` skips deleted lines in three places (:59, :94, :170). Change all three at once, run the guard, restore:

  ```bash
  cp crates/er-engine/src/ai/relocate.rs "$TMPDIR/relocate.rs.bak" && sed -i '' 's/if dl.line_type == LineType::Delete {/if dl.line_type != LineType::Add {/g' crates/er-engine/src/ai/relocate.rs; ~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pre_existing_finding_on_a_context_line; cp "$TMPDIR/relocate.rs.bak" crates/er-engine/src/ai/relocate.rs
  ```
  Expect the guard to fail. If it still passes, the mutation does not reach the path the guard uses; stop and report rather than counting the guard as proven.

## Gotchas & landmines
- **Folded context.** `parse_diff` folds context runs longer than 21 lines (diff.rs:428). At the default 10 lines of context a hunk's run between two changes is at most 20, so it is not folded; a tab with expanded context can fold, and a finding on a folded line reads as stale. Accept it; the ADR records it.
- **Promote to GitHub.** Promotion (`promote_finding_to_comment_impl`, `crates/er-desktop/src/commands.rs:9065-9145`) writes a local GitHub comment, then deletes the finding (ADR 0009); the push to GitHub happens later. GitHub's own PR diff has fewer context lines than er's, so GitHub may reject a comment on a far context line at push time, after the finding is gone. `TODO(executor): find how er handles a GitHub comment that GitHub rejects on push — search for "must be part of the diff" and for the push error path in crates/er-engine/src/app/state/github_sync.rs and crates/er-engine/src/sync.rs`. If the comment text survives locally with an error, record that in the ADR's Consequences. If it can be lost, stop and ask.
- **Shared analyze line.** `general_review_instructions_read_analyze` is used by triage and professor too; the new wording defers to each preamble's anchor block, which still says `+`/`-` only for them.
- **The arbiter does not see the flag (until task 07 lands).** Seeded runs dedupe expert findings into arena findings and import sets `outside_diff: false` (arena/import.rs:70). An arbiter-graded expert finding keeps its own `pre_existing` in `experts/<id>.json` because the overlay does not rewrite findings (ADR 0035); only arena imports lose it. Leave it here; task 07 Step 4 carries the flag into `ArenaFinding` and the arbiter prompt.
- **"tag" is a word to avoid.** `CONTEXT.md` lists it under Category. In code and docs say "Pre-existing" or "the `pre_existing` flag".
- `just comments-check` rejects comments citing a task or plan step.

## Decision log (forks pre-answered)
- **A new field; `outside_diff` keeps its meaning:** `outside_diff` keeps "where the anchor sits"; `pre_existing` is "who introduced the bug". Tasks 07, 08, 09, 10 and 18 read `pre_existing` (see the consistency note below).
- **Consistency pass (2026-10-17): who else touches this flag.**
  - 07 Step 4 copies it into `ArenaFinding` on the seeded path; Deep (arena import) still drops it. So this file's "drops `pre_existing`" consequence holds only until 07 lands.
  - 09 Step 2 reuses this file's `FlatFinding.pre_existing` (Step 6) instead of adding its own.
  - 10 Step 5 draws a "Security" group above the change's own findings in the same TUI panel and AI Review card. A security-lens finding that is also Pre-existing goes in the Security group and keeps this file's Pre-existing label.
  - 18 sets the flag on every Sweep finding and reuses this file's desktop pill styling. ADR 0006 and `crates/er-engine/src/CLAUDE.md` forbid a second meaning on an existing field.
- **Which anchors:** tagged context lines inside hunks only. No document anchors for findings.
- **Gate unchanged:** a Pre-existing finding passes or fails the gate on its confidence like any other. The fix for hiding is in the prompt and the `Informational` doc.
- **Caps:** 3 for the general review, 1 per expert lens, 0 for triage and professor, all inside the existing per-file and total caps.
- **Important / Nit mapping (recorded here for every later task; no task draws these labels yet, which is a decision left to the owner):** `pre_existing == true` → **Pre-existing**, whatever the severity. Otherwise `high` or `medium` → **Important** (P0 must fix, P1 should fix, per the preamble's severity block); `low` or `info` → **Nit**. This task does not draw Important/Nit labels.
- **Regressions in unchanged code** are the change's own findings: anchored on the `+`/`-` line that caused them, `pre_existing: false`.
- **Sorting:** within each group, keep the existing order (TUI: resolved, trust, hunk, line, severity; desktop: severity, trust, file).
- **Desktop color:** reuse the hairline pill styling; no new theme token.
- **Counting rule, both front ends:** every total keeps counting Pre-existing findings (TUI `Findings (V)`, desktop `high`/`med`/`low` from the snapshot and `countBySeverity` / `reviewFindingCounts` / `totalReviewFindings` in `aiReviewAgents.ts`, all unchanged). Where a list draws the Pre-existing group, it also shows their count on its own (TUI ` · P pre-existing`, desktop `Pre-existing (n)` heading). Excluding them from totals is task 08's call (blocking counts).

## Full verification
1. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine` → `test result: ok`, 0 failed.
2. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui` → `test result: ok`.
3. `~/.claude/scripts/locked.sh . cargo test -p er-desktop` → `test result: ok`.
4. `~/.claude/scripts/locked.sh desktop-ui bun run test` and `~/.claude/scripts/locked.sh desktop-ui bun run check` → pass, 0 errors.
5. `~/.claude/scripts/locked.sh . just clippy` and `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
6. `~/.claude/scripts/locked.sh . just docs-check` and `~/.claude/scripts/locked.sh . just comments-check` → exit 0.
7. Manual: in a scratch repo, commit a file with an obvious bug (an unchecked `unwrap()` on user input), then on a branch edit a line within 5 lines of it. Run a general review from the TUI (`just run`). Expect at most 3 findings marked `[pre-existing]` in their own group, and none of the branch's own findings marked.

## Rollback
Revert the commits. `review.json` files written with `pre_existing` still load on the old build (serde ignores unknown fields on `Finding`; confirm `Finding` has no `deny_unknown_fields`, it has none at c96dc0f12). Pre-existing findings then show as ordinary findings on context lines. Mark the ADR superseded rather than deleting it.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- After Step 1, always, until the owner approves the ADR and the caps.
- A promoted Pre-existing finding can be deleted while its GitHub comment fails to post (Gotchas, "Promote to GitHub").
- Any existing test other than the ones this file names has to change.

## Comments
