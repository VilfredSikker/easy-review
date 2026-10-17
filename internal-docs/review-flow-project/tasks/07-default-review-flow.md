# 07 — Default review flow: triage → Find → Verify → Rank

Status: needs-design
Idea: 12 in the original idea list (summarised in ../README.md)
Depends on: 02-arena-round1-rules.md (Deep reuses the arena's round-1 prompt). Step 4's `pre_existing` carry applies only once 06-pre-existing-tag.md has landed; skip that bullet if it has not.
Impact: H · Complexity: H · Size: XL
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.
>
> **Step 1 is a design step.** Write the design note, the ADR draft and the `CONTEXT.md` term, then STOP and ask the owner to approve them. Write no code before that approval.

## In plain terms

- **The problem.** er has five ways to start an AI review (triage, general, expert, arena, validate), and a person has to pick one, start it, then pick the next. The TUI has no arena and no arbiter at all.
- **Why it matters.** The point of er is to keep up with an agent that writes faster than a person reads. Choosing and chaining review passes by hand is the work a machine should do; the person's attention belongs on the findings that survive.
- **The fix.** One Review action. A cheap triage pass decides how deep to go (skip, Quick, Standard, or Deep) and which lenses to run. The finders run in parallel, an arbiter checks every finding against the code, and the survivors land in the review. Deep costs more, so it shows the estimate and waits for a click. The old buttons move under an Advanced group.
- **TL;DR.** Chain triage, the finders and the arbiter into one click, in the engine, so both the TUI and the desktop get it.

## Source

From the Code Review docs, https://code.claude.com/docs/en/code-review:

> 1. **Find**: several agents examine the diff and the surrounding code in parallel.
> 2. **Verify**: a verification step checks the candidate findings against the code and filters out false positives.
> 3. **Rank**: the findings that remain are deduplicated and ranked by severity.

> * **Quick**: a lighter pass. The fastest level, at the lowest cost.
> * **Standard**: balanced depth, speed, and cost. The default level.
> * **Deep**: the most thorough pass. Takes the longest and costs the most.

From the blog, https://claude.dev/blog/solving-code-review-at-anthropic/:

> Several agents look for bugs at the same time. Then a separate group votes on each finding, and it only gets posted if enough of them agree it's real.

> Deep is worth it where one missed bug costs more than the extra review time.

## Mission & definition of done

Add an engine-level **review flow**: a state machine that starts with triage, maps the triage verdict to a depth (skip, Quick, Standard, Deep) and a lens set, spawns the finders through the existing background queue, runs the arbiter over what they found, and reports where it is. Both front ends start it from one Review action and show its progress. Deep stops in an awaiting-confirmation state that carries the cost estimate until the person confirms. The old General, Expert and Arena entries sit under an Advanced group in the TUI AI Hub, the desktop AI Hub (`CommandPalette.svelte`) and the desktop AI review card.

- [ ] Step 1's design note, ADR draft and `CONTEXT.md` entry are approved by the owner (recorded under `## Comments` with the date).
- [ ] A pure reducer (no spawns, no disk) holds the flow's states and transitions, with unit tests for every transition.
- [ ] A pure function maps a `TriageReview` to a depth and a lens set, with a unit test per verdict.
- [ ] Standard runs the chosen expert lenses plus the general review in parallel, then one seeded arbiter pass whose verdicts land in `arbiter.json` (no accept step).
- [ ] Quick runs the general review, then the same seeded arbiter pass over the general findings.
- [ ] Deep computes a cost estimate, waits in `AwaitingConfirmation`, and starts an arena run with `confirm: true` only after the person confirms. Cancelling leaves nothing running.
- [ ] Every spawn goes through `spawn_background_*` or the arena (both take an **agent slot**, ADR 0021). Nothing in the flow calls `spawn_agent_prompt`.
- [ ] Every stage of one flow run uses the diff hash computed once at the start of that run.
- [ ] The flow advances from `App::poll_background_tasks`, so the TUI and the desktop both drive it with no front-end timer.
- [ ] TUI: the AI Hub's first item is "Review"; "Review work", "Specialized review" and "Validate review" sit under an "Advanced" header. Progress shows as a notification and in the background-task list.
- [ ] Desktop: the desktop AI Hub's first AI entry is "Review"; "Run review", "Run reviewers…", "Validate / re-anchor" and "Open AI review arena" sit under an "Advanced" group; the AI review card has a Review button above its arena block. The triage card's follow-up button starts the flow's next stage instead of opening the arena launcher.
- [ ] Nothing starts on PR refresh. `crates/er-desktop/src/auto_triage.rs` still queues nothing on refresh.
- [ ] `just docs-check`, `just comments-check`, `just clippy`, `just lint-ui`, `just test`, `just test-ui` pass.

**Risk:** high. It touches the agent spawn path, cost, and two front ends, and a wrong hash silently drops every arbiter verdict.

## Before you start (orientation)

- **Branch:** the current release branch; see `../README.md` "Branching". This is a feature.
- **Commands** (run one at a time, each through the lock):
  - Engine unit tests: `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow`
  - Seeded and arena tests: `~/.claude/scripts/locked.sh . cargo test -p er-engine seeded`
  - Engine headless build (the pure reducer must compile without `ui`): `~/.claude/scripts/locked.sh . just build-engine-headless`
  - TUI + engine suite: `~/.claude/scripts/locked.sh . just test`
  - Desktop backend: `~/.claude/scripts/locked.sh . just test-desktop`
  - Desktop UI: `~/.claude/scripts/locked.sh . just test-ui`, then `~/.claude/scripts/locked.sh . just check-ui`
  - Lint gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`
  - Docs: `~/.claude/scripts/locked.sh . just docs-check`, `~/.claude/scripts/locked.sh . just comments-check`
- **The map** (confirmed paths):
  - `crates/er-engine/src/ai/triage.rs` — `TriageReview`, `TriageVerdictPrimary` (`General | Expert | Arena | Professor | Skip`, :13-20), `load_triage_review` (:174), `triage_is_fresh` (:178). The depth mapping reads these.
  - `crates/er-engine/src/ai/prompts.rs` — triage verdict instructions (:1112-1135), prompt builders: `build_triage_review_prompt_prepared_diff` (:1250), `build_review_prompt_prepared_diff` (:451), `build_expert_review_prompt_prepared_diff` (:539), `build_validate_prompt_prepared_diff` (:1412), `build_arena_round1_prompt_agent` (:1823).
  - `crates/er-engine/src/ai/experts.rs` — `EXPERTS` (:45-102), `FindingCaps` (:9-31), `expert_by_id` (:104).
  - `crates/er-engine/src/arena/seeded.rs` — `dedupe_expert_findings` (:36), `contributing_lenses` (:88), `build_seeded_run` (:113). Tests at :315.
  - `crates/er-engine/src/arena/orchestrator.rs` — `DEFAULT_COST_LIMIT_USD` (:26), `default_arbiter_from_hub` (:79), `estimate_cost_usd` (:134), `build_arena_diff_preview` (:200), `start_arena_run` (:250) with the cost gate at :304, `SeededStartParams` (:418), `start_seeded_run` (:448) which writes `arbiter.json` at :530-531, `resolve_reviewers` (:1487).
  - `crates/er-engine/src/app/state/arena.rs` — `review_diff_hash` (:26), `App::arena_preview` (:106), `App::arena_start` (:119), `App::arena_start_seeded` (:173), `App::arena_accept_findings` (:312, refuses seeded runs).
  - `crates/er-engine/src/app/state/comments.rs` — `spawn_background_review` (:2808), `spawn_background_expert_review` (:2826), `spawn_background_triage_review` (:3018), `spawn_background_agent_task` (:3045), `poll_background_tasks` (:3599), `dispatch_pending_background_tasks` (:3755), `spawn_agent_prompt` (:2460, which the flow must not use). Queue tests and the `test_app` helper at :4028.
  - `crates/er-engine/src/app/state/mod.rs` — `AiActionKind` (:439), `HubItem` (:486-502), `HubAction` (:506), `set_ai_selection_override` (:5934), `open_ai_hub` (:6610), `open_verify_hub` (:6851, shows the header-item pattern).
  - `crates/er-tui/src/input/mod.rs` — hub dispatch (`HubAction::RunTriageReview` at :398), `execute_ai_action` (:571), `ensure_prepared_diff_for_action` (:1014). `crates/er-tui/src/main.rs:537` calls `app.poll_background_tasks()` every tick.
  - `crates/er-desktop/src/commands.rs` — `run_ai_review` (:3825), `spawn_scoped_reviewers` (:4424), `run_ai_validate` (:4518), the poll that calls `app.poll_background_tasks()` (:10550).
  - `crates/er-desktop/src/arena_commands.rs` — `arena_estimate` (:131), `arena_start` (:198), `arena_validate_findings` (:259). Commands are registered in `crates/er-desktop/src/main.rs` (`generate_handler!` at :1895) and allowed in `crates/er-desktop/permissions/app-commands.toml` (arena block :176-187).
  - `crates/er-desktop/src/snapshot.rs` — `active_arena_run` field (:537) and fill (:2866): the pattern for exposing flow state.
  - `desktop-ui/src/lib/triageCard.ts` — `followUpFor` (:110), tested in `desktop-ui/src/lib/triageCard.test.ts:118`. `desktop-ui/src/lib/components/TriageCard.svelte` `runFollowUp` (:72).
  - `desktop-ui/src/lib/components/CommandPalette.svelte` — `Group` union (:27), AI items (:126-230).
  - `crates/er-desktop/src/auto_triage.rs` — header (:1-5) says auto-dispatch on refresh is disabled. Keep it that way.
- **Conventions to follow.** A spawn that the engine queues, copied from `crates/er-desktop/src/commands.rs:4468-4476`:

  ```rust
  ReviewerKind::Expert(id) => {
      let mut prompt =
          prompts::build_expert_review_prompt_prepared_diff(scope, er_dir, id, diff_hash);
      if scoped_files {
          prompt = prompts::append_file_scope_if_present(prompt, er_dir);
      }
      app.spawn_background_expert_review(id, target.clone(), prompt, true)
  }
  ```

  A pure engine step that reads sidecars and writes nothing, with tests over a `tempdir` (`crates/er-engine/src/arena/seeded.rs:36`, tests from :315):

  ```rust
  pub fn dedupe_expert_findings(
      er_dir: &str,
      current_diff_hash: &str,
      review_hash: &str,
  ) -> Vec<ArenaFinding> {
  ```

  A TUI hub section header (`crates/er-engine/src/app/state/mod.rs:6858-6865`):

  ```rust
  items.push(HubItem {
      label: "── Packages ──".into(),
      hint: "".into(),
      description: "".into(),
      action: HubAction::Noop,
      is_header: true,
      enabled: false,
  });
  ```
- **Mental model.** Triage writes `triage.json` and reviews nothing (`CONTEXT.md` **Triage**). Ordinary AI actions spawn an agent CLI through the App's background queue (`spawn_background_*`), which holds an **agent slot** per process and finishes on `poll_background_tasks`. The agent writes its sidecar into the tab's **view bucket**: `review.json` for the general review, `experts/<id>.json` per lens. The seeded run (`start_seeded_run`) spawns no finders: it reads the expert sidecars already on disk, dedupes them, and makes one **arbiter** call on a supervisor thread, whose verdicts land in `arbiter.json` and are overlaid on the review at load (ADR 0035). The arena (`start_arena_run`) spawns its own reviewers, cross-checks them, runs the arbiter, and keeps its findings in its own store until a person accepts them (ADR 0024).
- **ADRs to read first:**
  - 0004 — sidecars are per view bucket; the flow must address its own bucket, never "the active tab".
  - 0006 — engine state is the UI contract; add explicit fields, never repurpose one.
  - 0021 — every spawn path that runs a review agent takes a slot. A new path is uncapped unless it acquires one.
  - 0023 — prompts are compiled into `prompts.rs`.
  - 0024 — arena findings need quorum and an explicit accept. Deep inside the flow touches this.
  - 0031 — er drives the agent CLI over sidecar files; no provider client.
  - 0035 — arbiter verdicts are an overlay in `arbiter.json`; `review.json` keeps its existing writers.
  - 0039 — triage judges reach from engine facts.
  - 0042 — the catalog owns preset models; never write presets back to config.

## Scope boundaries

- **In scope:** the design note, ADR and `CONTEXT.md` term; a pure flow reducer; the triage-to-depth mapping; an App driver that spawns stages and advances on poll; Quick, Standard and Deep; Deep's cost confirmation; the Review entry and the Advanced group in both front ends; the triage card's follow-up.
- **OUT of scope / DO NOT TOUCH:**
  - Starting anything on PR refresh, or re-enabling auto-triage (`crates/er-desktop/src/auto_triage.rs`).
  - Re-runs after new commits (task 08). The flow offers nothing automatically.
  - A shared finder prompt with the general review's rules for the arena (task 02).
  - Drawing Important / Nit labels. 06's Decision log records the mapping (`pre_existing` → Pre-existing; `high`/`medium` → Important; `low`/`info` → Nit), and no task in this package draws those labels yet. Rank in this task means the existing dedupe plus the arbiter's verdicts. Carrying 06's `pre_existing` flag into the seeded run is in scope (Step 4).
  - Security as a separate stage (task 10) and several providers per lens (task 11). Leave seams for them; build neither.
  - er-mcp (`crates/er-mcp`). It spawns no agents. See "When to STOP and ask".
  - Removing or renaming any existing action. They move under Advanced and keep working.
  - Changing ADR 0024's accept step without the owner's answer from Step 1.

## Steps

### Step 1 — Design note, ADR draft and CONTEXT.md term (then STOP)

- **Goal:** the owner approves the flow's shape before any code exists.
- **Why / invariant:** the flow chains three spawn mechanisms (background queue, seeded run, arena) and touches ADR 0024. A wrong shape is expensive to undo across two front ends.
- **Files:**
  - Create `internal-docs/review-flow-project/design/07-default-review-flow.md` (the design note; it is committed, so do not put it under `.scratch/`, which is gitignored).
  - Create `docs/adr/<next>-review-flow.md`. Take the next free number at the time you write it: `ls docs/adr | tail -1` and add one. Tasks 05, 06 and 08 also add ADRs, so do not assume 0044.
  - Edit `CONTEXT.md`: add the term under `## Language`, after **Arbiter**.
- **Do this:**
  1. Write the design note with the recommended shape from item 4. Mark each "Open question" for the owner.
  2. Write the ADR in the style of `docs/adr/0035-arbiter-verdict-overlay.md` (Context, Decision, Consequences). It records: the flow is an engine state machine advanced from `poll_background_tasks`; every stage of a run shares one diff hash; Verify is the arbiter overlay at every depth; Deep's place relative to ADR 0024 (as the owner answers).
  3. Propose this `CONTEXT.md` entry (the owner may rename it):

     ```markdown
     **Review flow**:
     The default AI review: triage picks a depth and the lenses, finders run,
     and an arbiter grades what they found, without further clicks. Deep waits
     for the reviewer to confirm its cost. A flow run is bound to the diff hash
     it started on.
     _Avoid_: pipeline, auto-review, full review
     ```

     And the depth word, as a second entry right after it:

     ```markdown
     **Depth**:
     How much a review flow spends: `quick`, `standard`, or `deep`, chosen by
     triage. Distinct from a **Tier**, which is a file's importance.
     _Avoid_: level, mode, effort
     ```

     `CONTEXT.md` already lists `level` under *Avoid* for **Tier**, so the code and docs say "depth" even though the Code Review docs say "level".
  4. Recommended shape for the design note:
     - **States:** `Triaging → (Skipped | Finding → Verifying → Done)`, with `AwaitingConfirmation` between `Triaging` and `Finding` for Deep, and `Failed { reason }` / `Cancelled` reachable from any running state.
     - **Mapping** (pure function over `TriageReview`):

       | Triage verdict | Depth | Lenses |
       |---|---|---|
       | `skip` | none, flow ends `Skipped` | — |
       | `general` | Quick | `general` |
       | `expert` | Standard | `general` + `verdict.experts` (minus `mentorship`), or `general` + `security` when the list is empty |
       | `arena` | Deep | `general` + `verdict.experts` (minus `mentorship`), at least one expert lens so the arena has two reviewers |
       | `professor` | Quick | `general`; the done state suggests Professor |

     - **Quick:** general review, then a seeded arbiter pass over the general findings.
     - **Standard:** general review and each expert lens in parallel through the background queue, then one seeded arbiter pass over the general and expert findings together.
     - **Deep:** one arena run (`start_arena_run`) whose reviewers are one `ReviewerRef` per lens, each with its own `agent_kind` (`general`, `expert:<id>`), all on the hub's default selection; three rounds and the arbiter. Task 11 later puts two or more providers on each lens.
     - **Verify** is the arbiter overlay at every depth (ADR 0035). Validate is left as an Advanced action.
  5. Open questions for the owner, each with the recommendation:
     - **Deep and the accept step (ADR 0024).** Recommend: keep the explicit accept. The flow ends Deep in `Done` with "N findings waiting to accept" and opens the arena results; nothing is imported on its own. The alternative (write Deep's verdicts as `arbiter.json`) needs an ADR 0024 amendment.
     - **Quick verify.** Recommend the seeded arbiter over general findings, the same call count as Validate. Validate rewrites `review.json` and drops findings it judges resolved; the arbiter writes an overlay and keeps the original claims (ADR 0035). Alternative: Validate, as the original idea list first proposed. Tasks 09, 10 and 14 assume the seeded arbiter; if the owner picks Validate, tell them 10 Step 4 stops (security findings at Quick would go unverified).
     - **Correctness at Standard.** `dedupe_expert_findings` reads only `experts/*.json`, so general findings in `review.json` never reach the arbiter today. Recommend extending the seeded input to include the general findings (Step 4). Alternative: Standard runs no general review.
     - **The term** ("Review flow", "Depth") and whether `Professor` maps to Quick.
     - **er-mcp.** Recommend: out of scope for this task; a later task decides between the `er-review` skill copying the stages with its own subagents, and an er-mcp tool that asks a running er to start the flow.
- **Watch out for:** do not write "level" as a defined term, and do not write "issue" for a finding. Do not add file tables or type listings to the ADR (`docs/agents/writing-docs.md`).
- **Verify:** `~/.claude/scripts/locked.sh . just docs-check` → exits 0.
- **If it fails:** a link or ADR reference does not resolve. Fix the path; never delete the reference to go green.

**STOP here and ask the owner to approve the design note, the ADR and the term.** Record the answers under `## Comments` with the date. Every later step assumes the recommended shape; where the owner chose differently, follow the owner and note the change.

### Step 2 — Pure flow reducer

- **Goal:** the flow's states, events and transitions as plain data with no spawns, no disk and no `App`.
- **Why / invariant:** transitions are the part most likely to be wrong, and a reducer can be tested without a fake provider. The reducer must compile without the `ui` feature so a later er-mcp path could reuse it.
- **Files:** create `crates/er-engine/src/ai/review_flow.rs`; add `pub mod review_flow;` and `pub use review_flow::*;` to `crates/er-engine/src/ai/mod.rs` next to the existing `pub mod triage;` (:14) and `pub use triage::*;` (:30). `ai/` is always compiled (`crates/er-engine/src/lib.rs:8`).
- **Do this:**
  - Define `FlowDepth { Quick, Standard, Deep }`, `FlowState` (the states from Step 1), and `FlowEvent` (`TriageDone { fresh: bool }`, `TriageFailed(String)`, `FinderDone { lens: String }`, `FinderFailed { lens: String, reason: String }`, `Confirmed`, `Declined`, `VerifyStarted { run_id: String }`, `VerifyDone`, `VerifyFailed(String)`, `Cancel`).
  - `pub struct ReviewFlow { pub diff_hash: String, pub depth: Option<FlowDepth>, pub lenses: Vec<String>, pub pending_finders: Vec<String>, pub failed_finders: Vec<(String, String)>, pub arena_run_id: Option<String>, pub cost_estimate_usd: Option<f32>, pub state: FlowState }`.
  - `impl ReviewFlow { pub fn apply(&mut self, event: FlowEvent) }`. Rules: an event that does not fit the current state is ignored (no panic). `Finding → Verifying` only when `pending_finders` is empty and at least one finder succeeded; all finders failed → `Failed`. `Cancel` from any running state → `Cancelled`. `TriageDone { fresh: false }` → `Failed("triage is stale")`.
  - Leave the depth decision to Step 3; `apply` takes the depth and lenses as already set.
- **Watch out for:** no `std::fs`, no `crate::app`, no `crate::arena` in this file. The `arena` module is behind `#[cfg(feature = "ui")]` (`crates/er-engine/src/lib.rs:11-12`).
- **Verify:**
  - Write tests first in a `#[cfg(test)] mod tests` at the bottom: one per transition, plus "ignored event leaves state unchanged", "all finders failed → Failed", "one finder failed, one ok → Verifying".
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
  - `~/.claude/scripts/locked.sh . just build-engine-headless` → exits 0.
- **If it fails:** a headless build failure means the file reached into a `ui`-gated module. Move that use out.

### Step 3 — Map triage to a depth and lenses

- **Goal:** `pub fn plan_for_triage(triage: &TriageReview) -> Option<(FlowDepth, Vec<String>)>` in `ai/review_flow.rs`, `None` for `skip`.
- **Why / invariant:** triage routes and does not review. The mapping is engine code so both front ends route the same way. The triage prompt and `triage.json` schema stay unchanged in this task.
- **Files:** `crates/er-engine/src/ai/review_flow.rs`; reads `TriageVerdictPrimary` (`crates/er-engine/src/ai/triage.rs:13-20`) and `TriageVerdict.experts` (:57-66).
- **Do this:** implement the table from Step 1 (as approved). Drop lens ids that `expert_by_id` (`crates/er-engine/src/ai/experts.rs:104`) does not know, and drop `mentorship` (it is positive-only, `experts.rs:96-101`). Always put `general` first. Deduplicate.
- **Watch out for:** the desktop's current fallback for an expert verdict with no experts is security (`desktop-ui/src/lib/triageCard.ts:115-121`). Keep the same fallback.
- **Verify:** one test per verdict, plus "unknown expert id dropped", "mentorship dropped", "arena with no experts still yields two lenses". `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
- **If it fails:** compare against the approved table, not against your memory of it.

### Step 4 — Let the seeded run grade general findings too

- **Goal:** the seeded arbiter pass can include the general review's findings, so Quick and Standard verify correctness findings.
- **Why / invariant:** `dedupe_expert_findings` reads only `experts/*.json` (`crates/er-engine/src/arena/seeded.rs:45`). The overlay already grades every finding in the merged review (`crates/er-engine/src/ai/loader.rs:353-358`), so verdicts keyed with `finding_key` over general findings will apply. `review.json` gains no writer (ADR 0035).
- **Files:**
  - `crates/er-engine/src/arena/seeded.rs:36` — `dedupe_expert_findings`.
  - `crates/er-engine/src/arena/seeded.rs:133-141` — `build_seeded_run` builds `agent_kind: Some(format!("expert:{lens}"))` for every contributing lens.
- **Do this:**
  - Add `pub fn dedupe_flow_findings(er_dir: &str, current_diff_hash: &str, review_hash: &str, include_general: bool) -> Vec<ArenaFinding>`. It reads `review.json` from `er_dir` the way `load_ai_state` does (`crates/er-engine/src/ai/loader.rs:218-222`: `read_sidecar` then `serde_json::from_str::<ErReview>`; there is no standalone review loader, and `read_sidecar` at :20 is private, so either make it `pub(crate)` or apply the same 10 MB guard inline), turns each general finding into an `ArenaFinding` with `raised_by: vec!["general"]` using `from_expert_finding` (:180) and `finding_key` (:58), only when `review.diff_hash` equals `current_diff_hash` or `review_hash`, then runs the same `propose_merge_candidates` + `collapse_groups` path. Keep `dedupe_expert_findings` as a thin call with `include_general: false` so its tests stay as they are.
  - In `build_seeded_run`, map the lens `general` to `agent_kind: Some("general".into())`. `agent_meta` knows `general` (`crates/er-engine/src/arena/agents.rs:26`) and does not know `expert:general`, so without this `resolve_reviewers` fails with "unknown agent_kind".
  - **Carry `pre_existing` through the seeded run and the arbiter (only if task 06 has landed; check with `grep -n "pub pre_existing: bool" crates/er-engine/src/ai/review.rs`).** Standard and Quick verify through this seeded path, and today it drops the flag that 06 Step 2 adds to `Finding`, so the arbiter could grade an old bug down to `informational` and the gate would hide it.
    - Add `#[serde(default)] pub pre_existing: bool` to `ArenaFinding` (`crates/er-engine/src/arena/model.rs:144`). Set it in `from_expert_finding` (`seeded.rs:180`) from `finding.pre_existing`, so it covers general and expert findings alike. `ArenaFinding` has no `Default`: `grep -rn "ArenaFinding {" crates | grep -v "pub struct"` lists 12 literals at c96dc0f12, and each needs `pre_existing: false` (or the copied value).
    - A group that `collapse_groups` merges is `pre_existing` only when every member is; one member that the change introduced makes the merged claim the change's own.
    - Make sure the flag reaches the arbiter: confirm the findings summary JSON passed to `build_arena_round3_prompt` (`prompts.rs:1897`, built near `orchestrator.rs:1347`) serializes `pre_existing`, and add one rule line to that prompt: "A finding with `pre_existing: true` reports a bug this change did not introduce. Judge only whether it is real; never lower its confidence because the code is old." This mirrors 06 Step 3.6's line for Validate.
    - The overlay does not rewrite findings (ADR 0035), so each finding keeps its own `pre_existing` in its sidecar; this bullet only stops the arbiter from grading blind.
    - Deep is not covered: the arena round-1 schema has no such field and `arena/import.rs:70` writes `outside_diff: false` with no flag. Leave that; record it in the ADR's Consequences.
    - Test: "a pre-existing expert finding stays pre-existing in the seeded run, and a merge with a non-pre-existing finding clears it", plus a prompt test that `build_arena_round3_prompt` contains `pre_existing: true`.
- **Watch out for:** a finding already merged into the review from an expert sidecar must not be counted twice. Read general findings from `review.json` on disk, where expert findings are not merged (they merge at load time only).
- **Verify:** add tests to `crates/er-engine/src/arena/seeded.rs` tests (:315), following `write_expert` (:326): "a general finding and an expert finding on the same claim collapse to one row raised by both", "general findings from another diff hash are skipped", "a seeded run with a general finding lists `general` as a reviewer". Show each fails before the change. `~/.claude/scripts/locked.sh . cargo test -p er-engine seeded` → all pass, including the existing ones unchanged.
- **If it fails:** if an existing seeded test fails, the thin wrapper changed behaviour. Restore it; do not edit the old test.

### Step 5 — App driver and Standard

- **Goal:** `App::start_review_flow()` runs triage, then Standard end to end, advancing on poll.
- **Why / invariant:** both front ends call `poll_background_tasks` (`crates/er-tui/src/main.rs:537`, `crates/er-desktop/src/commands.rs:10550`), so advancing there needs no front-end timer. Each flow run is bound to the view bucket and diff hash it started on (ADR 0004), never to whatever tab is active when a stage finishes.
- **Files:**
  - Create `crates/er-engine/src/app/state/review_flow.rs`; register it in `crates/er-engine/src/app/state/mod.rs` beside `pub mod arena;` (:1).
  - Add `pub review_flows: HashMap<String, crate::ai::ReviewFlow>` (keyed by `er_dir`) to `App`, next to `active_arena_runs` (`mod.rs:5555`). Every `App` constructor must initialise it; find them with `grep -n "active_arena_runs: std::collections::HashMap::new()" crates/er-engine/src/app/state/mod.rs` (5 sites today: :5713, :5761, :5801, :5842, :11338).
  - `crates/er-engine/src/app/state/comments.rs:3599` — call `self.advance_review_flows()` at the end of `poll_background_tasks`, after the block commented `// 4. Launch queued tasks while there's capacity.`, that is after `self.dispatch_pending_background_tasks();` (:3749).
  - `crates/er-engine/src/app/state/arena.rs:173` — `arena_start_seeded` reads `self.tab()`.
- **Do this:**
  1. `start_review_flow(&mut self, scope: &str) -> Result<()>`: `scope` is the git scope string `"branch"`, `"unstaged"` or `"staged"`. The TUI passes `app.tab().mode.fetch_scope()` (`crates/er-engine/src/app/state/mod.rs:181-186`, which maps PR Diff and Tour to `"branch"`); the desktop passes the result of `resolve_review_scope` (`crates/er-desktop/src/commands.rs:3766`). Refuse remote tabs with the same message style as `crates/er-tui/src/input/mod.rs:931-933`. Build the raw diff once (`tab.raw_diff_for_review(scope)`), call `crate::ai::prepared_diff::ensure_review_inputs(&er_dir, &raw, tab.repo_rules())` once (if task 05 has landed, the function takes a fourth `review_md: Option<&str>` argument, 05 Step 4: pass `crate::ai::review_md::collect(&tab.review_md_source(), &raw).as_deref()`), keep the returned hash as `ReviewFlow.diff_hash`, build a `BackgroundTaskTarget` (field list in `crates/er-desktop/src/commands.rs:3883-3892`), and spawn triage with `spawn_background_triage_review` and `build_triage_review_prompt_prepared_diff(scope, &er_dir, &hash)`. Refuse a second flow on the same `er_dir` while one is running.
  2. `advance_review_flows(&mut self)`: for each running flow, look at the background tasks for its target (`self.background_tasks` and `self.pending_background_tasks`, matching `task.kind` and `task.target`). Feed `FlowEvent`s to the reducer. When triage finishes, `load_triage_review(er_dir)` and `triage_is_fresh(&t, &flow.diff_hash)`, then `plan_for_triage`.
  3. Standard: spawn `spawn_background_review` for `general` (prompt `build_review_prompt_prepared_diff`) and `spawn_background_expert_review` per lens (prompt `build_expert_review_prompt_prepared_diff`), all with `prepared_diff: true` and the flow's hash. The queue applies the cap (ADR 0021).
  4. When all finders are done, start the seeded run. Add `App::arena_start_seeded_for(&mut self, inputs: SeededFlowInputs) -> Result<Option<String>>` that takes `er_dir`, `repo_root`, `branch_ref`, `base_branch`, `raw_diff`, `diff_hash`, `review_hash` explicitly, and make `arena_start_seeded` (:173) a wrapper that fills them from `self.tab()`. Add `pub include_general: bool` to `SeededStartParams` (`orchestrator.rs:418`), and in `start_seeded_run` replace the `dedupe_expert_findings` call (:454-458) with `dedupe_flow_findings(.., params.include_general)` from Step 4. The flow passes `true`; `arena_start_seeded` passes `false`, so the desktop's existing "Validate findings" action behaves as today. `Ok(None)` (nothing to verify) → `Done` with zero findings.
  5. When `self.arena_registry.get_status(&run_id)` reports `Complete` → `VerifyDone`; `Failed` → `VerifyFailed`; `Cancelled` → `Cancelled`; `None`, `Queued` or `Running { .. }` → wait. `get_status` returns `Option<RunStatus>` (`crates/er-engine/src/arena/registry.rs:84`), and `RunStatus` is `Queued | Running { round } | Complete | Cancelled | Failed` (`crates/er-engine/src/arena/model.rs:19-25`). A run released from the registry returns `None`; fall back to `load_run(&ArenaPaths::for_run(er_dir, run_id))?.status` before treating it as lost.
  6. `cancel_review_flow(&mut self, er_dir: &str)`: cancel queued tasks (`cancel_queued_background_task`), cancel the arena run (`arena_cancel`), set `Cancelled`.
  7. Expose `pub fn review_flow_for(&self, er_dir: &str) -> Option<&ReviewFlow>` for the front ends.
- **Watch out for:**
  - Do not call `spawn_agent_prompt` (`comments.rs:2460`). It is a tab command outside the background queue, and the flow could not see it finish.
  - Adding `include_general` to `SeededStartParams` breaks its two struct literals: `crates/er-engine/src/app/state/arena.rs:200` and `crates/er-engine/tests/seeded_arbiter.rs:124`. Set `include_general: false` in both.
  - Triage needs no effort override from the flow: the background launcher already forces `low` effort for task kind `triage` (`comments.rs:3211-3215`). It runs on the current provider and model selection, like the hub's "Triage branch".
  - `spawn_background_agent_task` rejects a second task with the same kind and target (`comments.rs:3060-3072`). If the person already started a general review by hand, the flow's spawn fails with "already running". Treat that as "finder in progress" and wait for it; do not fail the flow.
  - The desktop's `run_ai_review` filters review-ignore globs out of the diff before preparing it (`crates/er-desktop/src/commands.rs:3867-3870`); the TUI does not. The flow prepares the diff in the engine, so it does not filter. Note this in the design note; do not add the filter in this step. The comment on `review_diff_hash` (`crates/er-engine/src/app/state/arena.rs:17-25`) explains why a hash mismatch silently drops every verdict.
  - A finished background task stays in `background_tasks` for 8 seconds, then moves to `recent_background_tasks` (`comments.rs:3727-3745`). Decide "done" from the task's status, not from its presence.
  - `BackgroundTask.kind` for an expert is `crate::ai::expert_task_kind(id)` (`experts.rs:424`); for triage `crate::ai::triage_task_kind()` (`triage.rs:183`); for the general review `"review"` (`comments.rs:2815`).
- **Verify:**
  - Unit tests in `crates/er-engine/src/app/state/review_flow.rs`, with an App helper copied from `test_app` (`comments.rs:4028`) but with `config.agent.command = "true"` so spawns exit at once, and the `serial()` guard (:4024) because the slot pool is process-wide. Write `triage.json` and `experts/<id>.json` by hand into the bucket, call `poll_background_tasks` until the flow reaches `Verifying`, and assert the lenses spawned and the hash carried.
  - The arbiter half goes in a new integration test file `crates/er-engine/tests/review_flow_standard.rs`, one test only, using `ER_FAKE_ARENA_DIR` like `crates/er-engine/tests/seeded_arbiter.rs` (the variable is process-wide, which is why that file holds one test). Assert `arbiter.json` exists with the flow's hash and `review.json` is byte-identical before and after.
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
- **If it fails:** a flow stuck in `Finding` usually means the kind or target match is wrong. Print `background_task_snapshots()` in the failing test before changing the driver.

### Step 6 — Quick

- **Goal:** Quick runs the general review and then the seeded arbiter over its findings.
- **Why / invariant:** one Verify mechanism at every depth (as approved in Step 1). If the owner chose Validate instead, stop and ask how the flow should observe Validate finishing: today it runs through `spawn_agent_prompt` (`crates/er-desktop/src/commands.rs:4557-4559`, `crates/er-tui/src/input/mod.rs:1083`).
- **Files:** `crates/er-engine/src/app/state/review_flow.rs`.
- **Do this:** Quick is Standard with `lenses == ["general"]`. Reuse Step 5's code path; do not fork it.
- **Verify:** a driver test where triage says `general`: only the `review` task spawns, and the seeded run starts after it. `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
- **If it fails:** see Step 5.

### Step 7 — Deep with cost confirmation

- **Goal:** Deep estimates its cost, waits for the person, then starts one arena run.
- **Why / invariant:** the owner decided Deep always asks first. Today's gate only fires above `DEFAULT_COST_LIMIT_USD` (`crates/er-engine/src/arena/orchestrator.rs:304`); the flow asks at any cost. Arena findings reach `review.json` only through an explicit accept (ADR 0024) unless the owner changed that in Step 1.
- **Files:** `crates/er-engine/src/app/state/review_flow.rs`; `crates/er-engine/src/arena/orchestrator.rs:134` (`estimate_cost_usd`), :178 (`estimate_latency_sec`), :250 (`start_arena_run`).
- **Do this:**
  - On a Deep plan, build `reviewers: Vec<ReviewerRef>`, one per lens: `ReviewerRef { provider_id, model_id, agent_kind: Some(kind) }`, where `kind` is `"general"` or `format!("expert:{id}")` and the provider and model are the hub default (`config.ai_hub.default_provider` / `default_model`, as `crates/er-engine/src/arena/seeded.rs:122-131` reads them). `resolve_reviewers` reads each ref's own `agent_kind` (`orchestrator.rs:1500`), and round 1 passes it to the prompt (`orchestrator.rs:840-844`).
  - Compute `estimate_cost_usd(raw_diff.len(), &reviewers, Some(ARENA_ROUNDS_V1), None, &config.ai_hub)` and `estimate_latency_sec`, store them, and move to `AwaitingConfirmation`.
  - `confirm_review_flow(&mut self, er_dir: &str)` → start the run with `ArenaStartParams { reviewers, scope, files: None, rounds: None, arbiter: None, confirm: true, agent_kind: None, effort: None, title: Some("Review flow · Deep".into()) }`. Add an `App::arena_start_for(...)` that takes the flow's `er_dir`, `repo_root`, refs and diff explicitly, the way Step 5 did for the seeded run; `arena_start` (`arena.rs:119`) reads `self.tab()`.
  - `decline_review_flow` → `Cancelled`, nothing spawned.
  - On `Complete`, end in `Done` with the run id and the count of findings whose verdict is `Kept` or `Escalated`, so the front end can say "N findings waiting to accept".
- **Watch out for:**
  - `start_arena_run` refuses fewer than `MIN_QUORUM` (2) reviewers for more than one round (`orchestrator.rs:271-277`). The mapping guarantees two lenses; assert it before starting.
  - The arena reads its diff with `tab.raw_diff_for_arena` (`arena.rs:77`). Pass the flow's own raw diff so the hash matches.
  - Never match on the error text "exceeds limit" (the desktop store does at `desktop-ui/src/lib/stores/arena.svelte.ts:218`). The flow passes `confirm: true` after its own confirmation.
- **Verify:** reducer tests for `AwaitingConfirmation → Confirmed → Finding` and `Declined → Cancelled`; a driver test that a Deep plan spawns nothing before confirmation. `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
- **If it fails:** if `start_arena_run` bails on reviewers, print the refs; a lens id `agent_meta` does not know fails there (`crates/er-engine/src/arena/agents.rs:16`).

### Step 8 — TUI: Review entry and Advanced group

- **Goal:** the TUI AI Hub opens with "Review"; the old entries sit under "── Advanced ──".
- **Why / invariant:** the owner decided the old General, Expert and Arena entries move under Advanced and stay available. The TUI has no Arena entry today, so Advanced holds "Review work", "Specialized review" and "Validate review".
- **Files:** `crates/er-engine/src/app/state/mod.rs:6610` (`open_ai_hub`), `:439` (`AiActionKind`), `:506` (`HubAction`); `crates/er-tui/src/input/mod.rs:571` (`execute_ai_action`) and the `dispatch_hub_action` match (the `RunTriageReview` arm is at :398).
- **Do this:**
  - Add `HubAction::StartReviewFlow`, `HubAction::ConfirmReviewFlow`, `HubAction::CancelReviewFlow`.
  - First hub item: label "Review", description from the flow state ("Triage, then the depth it picks" when idle; "Deep: about $X — confirm" when awaiting confirmation, with the action `ConfirmReviewFlow`; "Running: <state>" with `CancelReviewFlow` while running).
  - Insert an `is_header: true` item "── Advanced ──" after "Triage branch", then move "Review work", "Specialized review" and "Validate review" below it, unchanged.
  - In `dispatch_hub_action`, call `app.start_review_flow(app.tab().mode.fetch_scope())`, `app.confirm_review_flow(&er_dir)`, `app.cancel_review_flow(&er_dir)`, and surface errors with `app.notify`.
  - When the flow changes state, `advance_review_flows` calls `self.notify(...)` with one line ("Review: Standard, 3 lenses", "Review: verifying 12 findings", "Review done: 7 kept, 5 dropped"). Notifications are the TUI's progress surface.
- **Watch out for:** no test asserts the AI Hub's item order today (the only caller is `crates/er-tui/src/input/normal.rs:484`, and the labels appear only in `open_ai_hub`). Header items are skipped by `overlay_next` / `overlay_prev` (`crates/er-engine/src/app/state/mod.rs:8339`, :8416) and drawn by `crates/er-tui/src/ui/overlay.rs:389`; nothing new is needed for the header itself.
- **Verify:** an engine test that `open_ai_hub` puts "Review" first and the three moved items after the Advanced header. `~/.claude/scripts/locked.sh . just test` → all pass. Manually: `just run` in a repo with a branch diff, open the AI Hub, run Review, watch the notifications reach "Review done".
- **If it fails:** a TUI with no real terminal cannot run; that is not a crash to debug (repo `CLAUDE.md`).

### Step 9 — Desktop: Review button, Advanced group, triage card

- **Goal:** the desktop starts the flow from one entry, shows its state, confirms Deep, and groups the old entries under Advanced.
- **Why / invariant:** engine state is the contract (ADR 0006): the desktop reads the flow through a new snapshot field and never re-derives it.
- **Files:**
  - `crates/er-desktop/src/arena_commands.rs` — new commands `start_review_flow` (taking `scope: String`, resolved with `resolve_review_scope` like `run_ai_review` at `commands.rs:3825`), `confirm_review_flow`, `cancel_review_flow`, each wrapped in `run_blocking` and bumping `desktop_revision`, like `arena_validate_findings` (`arena_commands.rs:258-271`).
  - `crates/er-desktop/src/main.rs:1895` (`generate_handler!`) and `crates/er-desktop/permissions/app-commands.toml` (next to `"arena_validate_findings"`, :180) — register and allow each command.
  - `crates/er-desktop/src/snapshot.rs:537` and :2866 — add `review_flow: Option<ReviewFlowSnapshot>` beside `active_arena_run`; `desktop-ui/src/lib/types.ts:654` — the matching TS type.
  - `desktop-ui/src/lib/components/CommandPalette.svelte:27` — the `Group` union is closed; add `"AI · Advanced"` (or the label the owner approved).
  - `desktop-ui/src/lib/triageCard.ts:110` and `desktop-ui/src/lib/components/TriageCard.svelte:72`.
- **Do this:**
  - Desktop AI Hub (`CommandPalette.svelte`): a new first AI item "Review" (id `ai-review-flow`) that calls `start_review_flow`. Move `ai-run-review` (:135), `ai-run-reviewers` (:143) and `ai-validate` (:183) to the Advanced group, and move `open-arena` (:473-478, today in group `"PR"`) there too.
  - The desktop has a second AI surface: the arena block in `desktop-ui/src/lib/components/AiReviewCard.svelte:240-300` ("+ New arena", "Single review", "Run as Arena" / "Promote to Arena"). Put a "Review" button that calls `start_review_flow` above it, and label the arena block "Advanced". Keep its buttons working.
  - A confirmation surface for Deep showing `cost_estimate_usd` and latency, with Confirm and Cancel buttons wired to the two commands. The arena launcher already formats an estimate (`desktop-ui/src/lib/components/arena/ArenaLauncher.svelte:283`, `est. $${estimate.cost_usd.toFixed(2)}`); copy that wording.
  - Triage card: when a flow for this bucket is `AwaitingConfirmation` or running, the follow-up button shows its state; otherwise it calls `start_review_flow`. Keep `followUpFor` for the Advanced path and keep its tests passing.
- **Watch out for:** `App.svelte:463` reopens the arena launcher from the arena overlay's "New run". Leave it; it is part of the Advanced arena path.
- **Verify:**
  - A Rust test in `crates/er-desktop` that the snapshot carries the flow's state, in the `mod tests` at `crates/er-desktop/src/snapshot.rs:4513` (for example `snapshot_inbox_hides_disabled_kinds_and_drops_them_from_unread` at :4519 builds a snapshot to assert on).
  - Bun tests for the desktop AI Hub ordering and the triage card's button state, next to `desktop-ui/src/lib/triageCard.test.ts`.
  - `~/.claude/scripts/locked.sh . just test-desktop`, `~/.claude/scripts/locked.sh . just test-ui`, `~/.claude/scripts/locked.sh . just check-ui`, `~/.claude/scripts/locked.sh . just lint-ui` → all exit 0.
  - Visual check: run Storybook on a spare port and look at the desktop AI Hub and the triage card with Playwright. In a worktree, symlink `node_modules` from the main checkout and run both unsandboxed (the shared `node_modules` hits EPERM in the sandbox).
- **If it fails:** a missing permission entry shows as a runtime "not allowed" error, not a compile error. Check `app-commands.toml`.

## Gotchas & landmines

- The seeded run spawns no finders. It reads sidecars already on disk (`crates/er-engine/src/arena/seeded.rs:4-8`). Standard is two mechanisms chained: the background queue for finders, then the arena registry for the arbiter. The driver watches both.
- `arena_start_seeded`, `arena_start` and `arena_preview` all read `self.tab()`. A flow whose stage finishes while the person is on another tab would review the wrong bucket. Steps 5 and 7 add explicit-input variants for that reason.
- The TUI's existing hub actions use `spawn_agent_prompt`, a tab command, for triage, expert and validate (`crates/er-tui/src/input/mod.rs:382-403`). The flow uses the background queue in both front ends.
- The arena store is per bucket (`ArenaPaths::for_run(er_dir, run_id)`). A Deep run started for bucket A must keep bucket A's `er_dir` even after a tab switch; record it in `active_arena_runs` under the flow's `er_dir`.
- Expert sidecars are one file per lens (`experts/<id>.json`). Two finders on the same lens in one bucket overwrite each other. Task 11 needs the arena for that, never the expert path.
- `dedupe_expert_findings` reads every expert sidecar whose hash matches, including ones from an earlier manual run on the same diff. That is intended: same diff, same claims.
- The registry's `notify` callback is a no-op in `App::default_arena_registry` (`mod.rs:5567-5568`). The TUI learns of arena progress only by polling `get_status`.
- `review.json` already has three writers (ADR 0035). Nothing in this task adds a fourth.

## Decision log (forks pre-answered)

- **If you are tempted to add a depth field to the triage prompt:** don't. Map the existing verdict in the engine (Step 3). `triage.json` is also read by the er-mcp kit and the triage card, and a schema change belongs in its own task.
- **If the flow could persist its state to a sidecar:** don't, in this task. State lives in `App.review_flows` in memory; a restart abandons a running flow, and the sidecars its stages wrote stay valid. A `flow.json` sidecar would be a new artifact with its own staleness rules.
- **If the person starts a finder by hand while the flow runs:** the flow treats the "already running" refusal as "finder in progress" (Step 5).
- **If a `review.json` already exists:** until task 08 lands, the flow overwrites it without asking. Once 08 lands, `start_review_flow` calls 08's `plan_next_round` first, and a re-run becomes a changes-only round (08 Step 6, "With task 07"). "Review work" asks first (`HubAction::PromptReview`, `crates/er-tui/src/input/mod.rs:405-417`) because it is a deliberate re-run; the one-click flow is the default action, and a confirm step would add a second click to every run. The previous review is lost exactly as when "Review work" is told to clear.
- **If a remote tab asks for the flow:** refuse with "check out the PR first", as triage and validate already do (`crates/er-tui/src/input/mod.rs:931-933`, `crates/er-desktop/src/commands.rs:4527-4531`).
- **If Deep has only one provider installed:** run it anyway with one reviewer per lens. Several providers per lens is task 11.
- **Mentorship lens:** never a flow finder. It is positive-only (`experts.rs:96-101`) and finds no defects.
- **Rank:** this task ranks by the arbiter's verdicts and the existing severity. Pre-existing is task 06's flag, carried through the seeded run in Step 4. Important / Nit labels are a mapping in 06's Decision log that no task draws yet.
- **Consistency pass (2026-10-17): what later tasks expect from this file.** Keep these names, or record the new ones under `## Comments` so the later tasks can find them.
  - Entry point `App::start_review_flow(scope)`; driver `App::advance_review_flows()`; the end of a run is the reducer reaching `FlowState::Done` after `FlowEvent::VerifyDone`. Task 08 Step 6 calls `plan_next_round` inside `start_review_flow` and `record_round` when the flow reaches `Done`.
  - `plan_for_triage` (10 adds the `security` lens to it), `dedupe_flow_findings` (10), `App::arena_start_seeded_for` and `App::arena_start_for` (11), `ReviewFlow` and `App::review_flow_for(er_dir)` (15 reads coverage from it).
  - There is no separate Quick prompt builder: Quick spawns `build_review_prompt_prepared_diff` through Step 5's path, then the seeded arbiter (Step 6). Task 14 relies on that.
  - Flow state lives on `App.review_flows`, keyed by `er_dir`, in memory only, so a `TabState` method cannot see it. Task 15 reads it through `review_flow_for`.
  - Task 06's `pre_existing` flag runs through the seeded path (Step 4), because 06 alone would leave seeded and arbiter runs dropping it.

## Full verification

1. `~/.claude/scripts/locked.sh . cargo test -p er-engine review_flow` → all pass.
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine seeded` → all pass, existing tests unchanged.
3. `~/.claude/scripts/locked.sh . just build-engine-headless` → exits 0.
4. `~/.claude/scripts/locked.sh . just test` → all pass.
5. `~/.claude/scripts/locked.sh . just test-desktop` → all pass.
6. `~/.claude/scripts/locked.sh . just test-ui` and `~/.claude/scripts/locked.sh . just check-ui` → exit 0.
7. `~/.claude/scripts/locked.sh . just clippy` and `~/.claude/scripts/locked.sh . just lint-ui` → exit 0. Never grow a baseline (ADR 0038).
8. `~/.claude/scripts/locked.sh . just docs-check` and `~/.claude/scripts/locked.sh . just comments-check` → exit 0.
9. Manual, TUI: in a repo with a small branch diff, AI Hub → Review. Expect notifications for triage, the depth, verifying, done; `arbiter.json` in the bucket; `review.json` unchanged by the arbiter.
10. Manual, desktop: desktop AI Hub → Review on a diff triage routes to `arena`. Expect the cost prompt; Cancel spawns nothing; Confirm starts one arena run.

## Rollback

Each step is its own commit. Revert in reverse order. Steps 2–4 add code nothing calls until Step 5; Steps 8–9 only add entries and move existing ones. The ADR and `CONTEXT.md` entry revert with Step 1's commit. No sidecar format changes, so no stored data needs migrating.

## When to STOP and ask

- After Step 1, always, for approval.
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- The owner wants Deep's findings to reach the review without the accept step. That changes ADR 0024 and needs its own ADR amendment first.
- The owner wants the flow in er-mcp. er-mcp spawns no agents (`crates/er-mcp/src` has no agent spawn; the calling agent is the reviewer). The two options are a skill that copies the stages with its own subagents, or an er-mcp tool that asks er to start the flow; both are product decisions.
- The owner wants anything to start on PR refresh or on new commits. The decision on record is one click.
- Standard on a real diff queues long enough that the flow looks hung (Standard is general + lenses + arbiter, with a default cap of 3 per ADR 0021). Ask before changing any cap.

## Comments
