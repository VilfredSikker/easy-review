# 10 — Security as its own stage

Status: ready-for-agent
Idea: 16 in the original idea list (summarised in ../README.md)
Depends on: 07-default-review-flow.md (the review flow must exist). Task 09 (09-answer-every-finding.md) is optional: its part of Step 5 runs only if it has landed.
Impact: M · Complexity: L · Size: S
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.
>
> Task 07 names things this task builds on (`ReviewFlow`, `plan_for_triage`, `dedupe_flow_findings`, `App::advance_review_flows`). Before Step 1, confirm they exist: `grep -rn "fn plan_for_triage\|fn dedupe_flow_findings\|fn advance_review_flows" crates/er-engine/src`. If any is missing or named differently, read task 07's `## Comments` for what was built, and stop and ask if it still does not match.

## In plain terms

- **The problem.** Security is one expert lens among eight. It runs only when someone picks it, or when triage happens to name it, and its findings sit in the same list as everything else.
- **Why it matters.** A security mistake costs more than most bugs, and a person skimming a long list of findings can miss the one that leaks a credential.
- **The fix.** Whenever triage sees anything sensitive (auth, credentials, personal data, payments, a secret in a file), the review flow always runs the security lens, at every depth, even when triage would otherwise skip review. Security findings are shown in their own group, so they are never lost among the rest.
- **TL;DR.** Sensitive changes always get a security pass, and its findings get their own section.

## Source

From the blog, https://claude.dev/blog/solving-code-review-at-anthropic/:

> Security gets its own automated review too. A PR can't merge while any finding from the bug review or the security review is still open. You either fix it or close the thread with a short reason, which is how you clear a false positive.

> If a later commit fixes the issue, the finding clears on its own.

From the Code Review docs, https://code.claude.com/docs/en/code-review:

> Several agents read each change alongside the surrounding code, looking for issues such as logic errors, security vulnerabilities, or broken edge cases.

## Mission & definition of done

Triage records whether a diff touches anything sensitive, with a reason a person can check. The review flow's plan adds the `security` lens whenever that signal is set, at every depth, and runs a security-only Quick pass when triage would otherwise skip. Both front ends show findings raised by the security lens in their own group. If task 09 has landed, open security findings count toward "all answered".

- [ ] `triage.json` can carry a `sensitive` block; old files and malformed blocks load as "not flagged" without dropping the rest of the triage.
- [ ] The triage prompt asks for the block and says what counts as sensitive.
- [ ] `plan_for_triage` adds `security` when the triage is sensitive, at Quick, Standard and Deep, and turns a sensitive `skip` into a Quick pass with only the `security` lens.
- [ ] At Quick, the security findings go through the same seeded arbiter pass as the general findings.
- [ ] TUI and desktop show findings whose raisers include `security` in a separate group, above the others.
- [ ] The triage card shows a "sensitive" pill with the reasons on hover.
- [ ] If task 09 has landed: open security findings count toward its "all answered" state, and are counted separately.
- [ ] `just test`, `just test-ui`, `just clippy`, `just lint-ui`, `just docs-check`, `just comments-check` pass.

**Risk:** ordinary. A wrong sensitivity call costs one extra agent run, or misses a pass the person can still start by hand.

## Before you start (orientation)

- **Branch:** the current release branch; see `../README.md` "Branching".
- **Commands** (one at a time, each through the lock):
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine triage`
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow`
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine prompts`
  - `~/.claude/scripts/locked.sh . just test`, `~/.claude/scripts/locked.sh . just test-ui`, `~/.claude/scripts/locked.sh . just check-ui`
  - `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`
- **The map** (confirmed paths):
  - `crates/er-engine/src/ai/experts.rs:46-52` — the `security` expert (`id_prefix: "sec"`); `FindingCaps::expert()` at :25-31 caps experts at 2 per file, 10 in total.
  - `crates/er-engine/src/ai/triage.rs` — `TriageReview` (:22-42), the lenient `reach` field (:39-41) and `lenient_reach` (:149-160), the pattern to copy. Tests: `triage_without_reach_loads_as_unknown` (:202), `malformed_reach_keeps_the_rest_of_the_triage` (:230), `deserialize_sample_triage_json` (:263).
  - `crates/er-engine/src/ai/prompts.rs` — `triage_lens_instructions` (:1112-1135), `reach_instructions` (:1142-1158), `triage_output_section` (:1161-1201). Prompt tests: `triage_and_review_prompts_carry_reach_rules` (:2541).
  - `crates/er-engine/src/ai/review_flow.rs` — `plan_for_triage` (added by task 07).
  - `crates/er-engine/src/ai/review.rs` — `Finding.lens` (:279), `Finding.category` (:283), `Finding.raised_by` (:289), `Finding::raisers` (:405), `Finding::named_raisers` (:419).
  - `crates/er-tui/src/ui/panel.rs` — `render_file_findings` (:410), `push_finding` (:546).
  - `crates/er-desktop/src/snapshot.rs` — `TriageSnapshot` (:1321), finding snapshot `lens_category` (:1158) and its fill (:4287-4289).
  - `desktop-ui/src/lib/triageCard.ts` — `summaryPills` (:57); `desktop-ui/src/lib/components/AiReviewCard.svelte` — the findings list (`{#each ordered as finding}` at :485).
- **Conventions to follow.** A lenient optional block in `triage.json` (`crates/er-engine/src/ai/triage.rs:149-160`):

  ```rust
  /// A malformed `reach` block degrades to unknown rather than failing the parse:
  /// `load_triage_review` drops the whole file on any error, and losing the
  /// verdict over one optional block is the worse trade.
  fn lenient_reach<'de, D>(deserializer: D) -> Result<TriageReach, D::Error>
  where
      D: serde::Deserializer<'de>,
  {
      use serde::Deserialize as _;
      let value = Option::<serde_json::Value>::deserialize(deserializer)?;
      Ok(value
          .and_then(|v| serde_json::from_value::<TriageReach>(v).ok())
          .unwrap_or_default())
  }
  ```

  A guard claim only counts with evidence (`triage.rs:131-136`, `TriageGuard::is_evidenced`). The sensitive block follows the same rule.
- **Mental model.** Triage classifies and routes; it does not review (`CONTEXT.md` **Triage**). The review flow (task 07) turns a triage into a depth and a lens set with `plan_for_triage`, spawns each lens through the background queue, and grades the results with one seeded arbiter pass whose verdicts land in `arbiter.json`. A finding's **lens** is who produced it; its **category** is what kind of defect it is (`CONTEXT.md`, told apart on purpose). A finding merged from several producers lists them all in `raised_by`.
- **ADRs to read first:**
  - 0039 — reach is judged by the agent from engine facts, and a guard counts only with `path:line` evidence. The sensitive block copies that rule.
  - 0035 — the arbiter's verdicts are an overlay; nothing here writes `review.json`.
  - 0023 — prompts are compiled into `prompts.rs`, and asserted on by tests in the same module.
  - 0006 — engine state is the UI contract; add a field, never repurpose one.

## Scope boundaries

- **In scope:** the `sensitive` block in triage (schema, prompt, lenient load), the plan change, grouping security findings in both front ends, the triage-card pill, and (only if task 09 landed) the "all answered" count.
- **OUT of scope / DO NOT TOUCH:**
  - Blocking merge on open security findings. er never merges on its own, and the README's decisions forbid bot approvals.
  - Closing fixed security findings on the next changes-only run (task 08).
  - Task 12's People needed label, its `needs_person` triage field and its `[people_needed.<repo>].always_human` paths. 12 does not touch `plan_for_triage`; keep `plan_for_triage` taking only the triage. Reading always-a-person paths into the security decision would be a later change.
  - The security expert's prompt, id, or caps (`experts.rs:46-52`, `FindingCaps::expert()`).
  - A second security prompt or a new lens id. The stage is the existing `security` lens.
  - Building task 09's "all answered" state if it does not exist.

## Steps

### Step 1 — Add the `sensitive` block to triage

- **Goal:** `TriageReview` can carry a sensitivity call with evidence, and old or malformed files still load.
- **Why / invariant:** `load_triage_review` drops the whole file on any parse error (`read_triage_sidecar`, `triage.rs:165-176`), so a new block must never make a triage unreadable. A sensitivity claim without evidence counts for nothing, as a guard does (ADR 0039).
- **Files:** `crates/er-engine/src/ai/triage.rs`.
- **Do this:**
  1. Write the tests first, next to `malformed_reach_keeps_the_rest_of_the_triage` (:230): "triage without sensitive loads as not flagged", "malformed sensitive keeps the rest of the triage", "a reason without evidence does not count", "an evidenced reason flags the triage". Run them and show they fail to compile or fail.
  2. Add:

     ```rust
     /// Something in the diff a security pass must look at. Counts only with
     /// `evidence`, the `path:line` that shows it.
     #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
     pub struct TriageSensitiveReason {
         #[serde(default)]
         pub kind: String,
         #[serde(default)]
         pub evidence: String,
         #[serde(default)]
         pub note: String,
     }

     #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
     pub struct TriageSensitive {
         #[serde(default)]
         pub reasons: Vec<TriageSensitiveReason>,
     }
     ```

     and on `TriageReview`, after `reach`:

     ```rust
     #[serde(default, deserialize_with = "lenient_sensitive")]
     pub sensitive: TriageSensitive,
     ```

     with `lenient_sensitive` written like `lenient_reach`. Add `impl TriageSensitive { pub fn is_flagged(&self) -> bool }` that is true when at least one reason has non-blank `evidence`.
- **Watch out for:** every struct literal of `TriageReview` in the repo needs the new field. Find them: `grep -rn "TriageReview {" crates` (at c96dc0f12: `crates/er-engine/src/sidecar_upload.rs:565`, `crates/er-engine/src/sidecar_summary.rs:328` and `:438`, per 12 Step 3). Tasks 12 (`needs_person`) and 15 (`intent`, `pattern_setting`) add their own optional blocks after `reach` in the same struct, prompt and schema. The four fields do not collide; if one of them has landed first, add yours beside it and keep theirs.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine triage` → the four new tests pass, the existing ones are unchanged and pass.
- **If it fails:** a failing old test means the field is not `#[serde(default)]` or the lenient wrapper errors. Fix the field; never edit the old test.

### Step 2 — Ask triage for it

- **Goal:** the triage prompt asks for `sensitive` and defines it.
- **Why / invariant:** prompts are compiled in and asserted on in the same module (ADR 0023).
- **Files:** `crates/er-engine/src/ai/prompts.rs` — `triage_lens_instructions` (:1112), `triage_output_section` (:1161); `crates/er-engine/src/sidecar_specs.rs` — `triage_schema` (:42), whose `reach` block (:90-110) is the JSON schema the er-mcp kit hands an external agent.
- **Do this:**
  - In `triage_lens_instructions`, add a deliverable at the end of the numbered list, as the next free number (12 Step 3 appends `needs_person` as item 6 and 15 Step 2 appends intent and pattern-setting; appending keeps every task's numbering valid whichever lands first): "`sensitive` — list each place the diff touches authentication or authorization, credentials or secrets, personal data, payments, cryptography, or code that runs with elevated rights. Each entry needs `kind`, `evidence` (`path:line`) and a one-line `note`. Leave `reasons` empty when nothing applies; a guess without evidence counts for nothing."
  - In the JSON example in `triage_output_section`, add after `"reach": {…}`:

    ```json
    "sensitive": {
      "reasons": [
        { "kind": "credentials", "evidence": "src/auth/token.rs:42", "note": "reads the API token from env" }
      ]
    }
    ```

    Remember the `{{` / `}}` escaping that `format!` needs in that string.
  - Mirror the block in `triage_schema` (`crates/er-engine/src/sidecar_specs.rs:42`), after `"reach"`: `"sensitive": { "type": "object", "properties": { "reasons": { "type": "array", "items": { "type": "object", "properties": { "kind": {"type": "string"}, "evidence": {"type": "string", "description": "path:line"}, "note": {"type": "string"} } } } } }`. Without it, a triage written through er-mcp never carries `sensitive`, and the flow treats it as not flagged.
  - Add a test like `triage_and_review_prompts_carry_reach_rules` (:2541): the prepared triage prompt contains `"sensitive": {` and the phrase "a guess without evidence counts for nothing".
- **Watch out for:** do not touch the general review prompt or the expert prompts.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine prompts` → all pass. `~/.claude/scripts/locked.sh . cargo test -p er-engine sidecar_specs` → all pass, including `all_kinds_have_schemas_and_prompts` (`sidecar_specs.rs:550`).
- **If it fails:** a `format!` panic or compile error is the brace escaping.

### Step 3 — Always add the security lens when sensitive

- **Goal:** `plan_for_triage` puts `security` in the lens set whenever `triage.sensitive.is_flagged()`, at every depth.
- **Why / invariant:** the blog runs security on every PR; this runs it whenever triage has evidence it matters. A sensitive change is never skipped.
- **Files:** `crates/er-engine/src/ai/review_flow.rs` (`plan_for_triage`, from task 07).
- **Do this:**
  - Tests first, one per case: `general` + sensitive → Quick, lenses `["general", "security"]`; `expert` with `["testing"]` + sensitive → Standard, lenses include `security` once; `arena` + sensitive → Deep with `security`; `skip` + sensitive → Quick, lenses `["security"]`; `skip` without sensitive → `None` (unchanged); a sensitive reason without evidence → no change from the unflagged plan.
  - Implement: after the verdict mapping, if flagged, insert `"security"` if absent; for `skip`, return `Some((FlowDepth::Quick, vec!["security".into()]))`.
- **Watch out for:** keep `general` first in the list, then `security`, then the rest, so the order the front ends show is stable.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass, the task 07 tests unchanged.
- **If it fails:** if a task 07 test now fails, the new rule leaked into the unflagged path.

### Step 4 — Make sure Quick verifies the security findings

- **Goal:** at Quick, findings from `experts/security.json` reach the seeded arbiter with the general findings.
- **Why / invariant:** Validate reads and rewrites only `review.json` (`crates/er-engine/src/ai/prompts.rs:1428`, "Read `{safe_output_dir}/review.json`"), so an expert sidecar is never validated by it. The flow's seeded pass reads `experts/*.json` (`crates/er-engine/src/arena/seeded.rs:45`) and, after task 07 Step 4, the general findings too.
- **Files:** `crates/er-engine/src/app/state/review_flow.rs` (task 07's driver).
- **Do this:** confirm that the Quick path spawns every lens in the plan, not only `general`. If task 07 hard-coded Quick to the general review alone, change it to spawn each lens in `lenses` exactly as Standard does. Add a driver test: Quick with `["general", "security"]` spawns a `review` task and an `expert:security` task, and the seeded run starts after both.
- **Watch out for:** if task 07's owner answer made Quick verify with Validate instead of the seeded arbiter, STOP: security findings at Quick would go unverified.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
- **If it fails:** see task 07 Step 5's "If it fails".

### Step 5 — Show security findings apart, and count them (both front ends)

- **Goal:** findings raised by the `security` lens appear in their own group above the others; if task 09 landed, they count toward "all answered".
- **Why / invariant:** a security finding must not be lost in a long list. Grouping keys on the lens (who raised it), never on category; `CONTEXT.md` keeps the two apart, and the stage is the security pass.
- **Files:**
  - `crates/er-engine/src/ai/review.rs` — add `pub fn is_security_stage(&self) -> bool { self.raisers().contains(&"security") }` on `Finding` next to `named_raisers` (:419), with a test next to `lens_category_tag_names_every_raiser` (:1895).
  - `crates/er-tui/src/ui/panel.rs:410` (`render_file_findings`) — render security findings first, under a "Security" sub-heading. Task 06 Step 5 already splits this list into the change's own findings and a "Pre-existing" group drawn last. Order the groups Security, then the change's own, then Pre-existing. A security finding that is also `pre_existing` goes in the Security group and keeps 06's `[pre-existing]` label, so it is neither lost nor read as the author's fault.
  - `crates/er-desktop/src/snapshot.rs:4287` — add `security_stage: bool` to the finding snapshot (beside `lens_category`), filled from `is_security_stage()`; the matching TS field in `desktop-ui/src/lib/types.ts`.
  - `desktop-ui/src/lib/components/AiReviewCard.svelte:485` — render `ordered.filter(f => f.security_stage)` first under a "Security" heading, then the rest. If task 06 landed, the card already renders `splitPreExisting(ordered)` (06 Step 6.3–6.4, `desktop-ui/src/lib/aiReviewAgents.ts`) as two groups through a shared row `{#snippet}`. Take security findings out first, then split the remainder with `splitPreExisting`, and reuse the same snippet for the Security group. Same group order and rule as the TUI bullet.
  - `crates/er-desktop/src/snapshot.rs:1321` (`TriageSnapshot`) — add `sensitive: Vec<TriageSensitiveSnapshot>` with only evidenced reasons; `desktop-ui/src/lib/triageCard.ts:57` (`summaryPills`) — add a "sensitive" pill (tone `"warning"`) whose `title` lists `kind · evidence`. Add a case to `desktop-ui/src/lib/triageCard.test.ts`.
  - Task 09, only if landed (`grep -n "pub fn open_findings" crates/er-engine/src/ai/review.rs`): 09 Step 1 defines `pub struct OpenFindings { must_fix, nits, pre_existing }` with `AiState::open_findings()` and `AiState::all_answered()` in `crates/er-engine/src/ai/review.rs`, and 09 Step 2 adds `OpenFindingsSnapshot` as `AiSnapshot.open_findings` in `crates/er-desktop/src/snapshot.rs`. Add `pub security: usize` to both, counting open findings where `is_security_stage()` is true, and show it next to 09's count. Security findings are already inside `must_fix`/`nits`, so `all_answered()` needs no change: it is false while any of them is open. If the grep finds nothing, skip this item and write "09 not landed; security count deferred" under `## Comments`.
- **Watch out for:** a finding the arbiter dropped or merged is hidden today (`ArbiterEffect::hidden`, `crates/er-engine/src/ai/arbiter.rs`). Group only what is shown; do not resurrect hidden findings into the security group.
- **Verify:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine is_security_stage` → passes.
  - `~/.claude/scripts/locked.sh . just test` → all pass.
  - `~/.claude/scripts/locked.sh . just test-ui` and `~/.claude/scripts/locked.sh . just check-ui` → exit 0.
  - Manual, desktop: a branch whose diff adds a hard-coded token. Run Review. Expect the "sensitive" pill on the triage card, an `expert:security` task in the background list, and a "Security" group above the other findings.
- **If it fails:** a missing TS field shows as `svelte-check` errors; add it to `types.ts` rather than casting.

## Gotchas & landmines

- `domains` in `triage.json` is free text (`prompts.rs:1122`, "e.g. auth, api, tests"). Do not derive sensitivity from it: the words vary between runs and carry no evidence.
- Expert finding ids are positional (`sec-1`, `sec-2`, ADR 0035). Never key the security group or a count on the id; use `raisers()`.
- A finding merged from `general` and `security` has both in `raised_by`. It belongs in the security group (it was raised by the security lens) and must appear once, not twice.
- The security expert's caps (2 per file, 10 total) still apply. A sensitive PR with many sites may show at most 10 security findings; that is today's behaviour, and changing caps is out of scope.
- `dedupe_expert_findings` reads every expert sidecar whose hash matches, including an `experts/security.json` from an earlier manual run on the same diff. The flow's own spawn overwrites it, which is fine.

## Decision log (forks pre-answered)

- **Lens or category for "shown apart":** lens. The stage is the security pass, so the group is "what the security pass raised". A `general` finding with category `security` stays in the general list with its category tag. If the owner wants category instead, stop and ask.
- **Sensitive `skip`:** run a security-only Quick pass. A docs-only diff can still commit a secret.
- **Evidence rule:** a reason without `evidence` does not flag the triage, matching how a guard counts (ADR 0039).
- **Where the sensitive call comes from:** triage only. Declared always-a-person paths are task 12's `[people_needed.<repo>]` config, and this task does not read them.
- **`sensitive` versus 12's `needs_person` (consistency pass, 2026-10-17):** both are triage blocks about changes that touch credentials, money or personal data, and they serve different ends. `sensitive` decides whether the security lens runs. `needs_person` feeds 12's People needed label. Keep them as two fields and do not derive one from the other. Whether a flagged `sensitive` block should also raise the People needed label to at least one person is a decision for the owner; neither task does it.
- **Grouping next to 06's Pre-existing group (consistency pass, 2026-10-17):** order is Security, then the change's own findings, then Pre-existing. A security-lens finding that is also `pre_existing` sits in the Security group and keeps 06's Pre-existing label.
- **If task 09 has not landed:** skip its count and record that in `## Comments`. Do not build "all answered" here.

## Full verification

1. `~/.claude/scripts/locked.sh . cargo test -p er-engine triage` → all pass.
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine prompts` → all pass.
3. `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
4. `~/.claude/scripts/locked.sh . just test` → all pass.
5. `~/.claude/scripts/locked.sh . just test-desktop` → all pass.
6. `~/.claude/scripts/locked.sh . just test-ui` and `~/.claude/scripts/locked.sh . just check-ui` → exit 0.
7. `~/.claude/scripts/locked.sh . just clippy` and `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
8. `~/.claude/scripts/locked.sh . just comments-check` → exits 0 (no task numbers in code comments).

## Rollback

One commit per step; revert in reverse. The `sensitive` block is optional and lenient, so `triage.json` files written with it still load after a rollback (serde ignores the unknown field).

## When to STOP and ask

- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- Task 07's flow, `plan_for_triage` or `dedupe_flow_findings` is missing or shaped differently than this file assumes.
- Quick verifies with Validate instead of the seeded arbiter (task 07's owner answer). Security findings at Quick would be unverified.
- The owner wants security findings to block anything (approve, merge, promote). That conflicts with "no bot approvals".
- The owner wants the group keyed on category instead of lens.

## Comments
