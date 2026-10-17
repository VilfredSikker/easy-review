# 12 — People needed label for each PR

Status: ready-for-agent
Idea: 7 in the original idea list (summarised in ../README.md)
Depends on: —
Impact: M · Complexity: M · Size: M
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms

- **The problem.** er judges risk, importance and reach file by file inside one
  diff. Nothing tells a reviewer, from the PR list, whether a PR needs zero, one
  or two people to look at it.
- **Why it matters.** A reviewer with twenty open PRs spends attention evenly.
  A label that says "two people needed" on the auto-updater change and "no
  person needed" on a docs typo tells them where to start.
- **The fix.** A small label on each PR: *no person*, *one person*, *two people*,
  or *unknown*. The engine works it out from the triage, from paths the user
  declared as "always a person" (CI workflows, auth, billing, the updater), from a
  new triage field that says "this one needs a person anyway", and from a
  per-repo list of trusted authors. Shown on the desktop triage card, the
  sidebar's PR rows, review-request items in the inbox, the TUI triage summary,
  and the MCP `prs_query` result. It blocks nothing, approves nothing and
  reorders nothing.
- **TL;DR.** A display-only "people needed" label per PR, computed by the engine
  from triage plus declared config.

**Why the label is not called a "tier".** `CONTEXT.md` already defines **Tier**
as a file's importance (`foundational`, `normal`, `isolated`). Reusing the word
for a PR label would give one word two meanings. This task names the concept
**People needed**. The file name (and the README's row title) keep "risk tier" so the
README link still resolves; nothing in code, UI text or docs uses "tier" for it.

## Source

From https://claude.dev/blog/solving-code-review-at-anthropic/ ("Human approval,
scaled by risk"):

> | Low | Docs, copy, internal-only features, additive changes behind a flag | None required once automated review and security checks are green |
> | Medium | Ordinary product changes, and changes to sensitive areas such as caching, the auto-updater, or system prompts | One person |
> | High | The highest-risk code, plus a small set of paths that always need two people | Two people |

> A second classifier asks whether this particular change needs another person
> even when its tier says it doesn't. Some paths always need a person, anything
> owners have listed as sensitive and any CI workflow file.

> Only authors on the repo's trusted list get the lighter paths, mostly the team
> that owns it. Everyone else needs a second person to approve.

## Mission & definition of done

Add a pure engine function that turns a triage, the changed paths, the PR author
and the repo's declared rules into a **People needed** label with its reasons.
Add the config it reads, the triage field it reads, and show the label in both
front ends and in MCP. The label is display only: it changes no score, no sort
order, no button state, and nothing is sent to GitHub.

- [ ] `CONTEXT.md` defines **People needed**, with `tier` and `risk tier` under *Avoid*.
- [ ] An ADR records where the label is computed and why it is display only.
- [ ] `[people_needed.<repo>]` in the global config holds `trusted_authors` and an `always_human` path table. A malformed value in it costs that entry only, never the whole config.
- [ ] `triage.json` may carry `needs_person: { needed, reason }`; old files and malformed blocks load as "not stated".
- [ ] The engine function follows the decision table in Step 4 exactly, proven by a table-driven test.
- [ ] A missing, unreadable or `info` risk never produces "no person".
- [ ] Desktop triage card shows the label as the first pill; sidebar "to review" PR rows and review-request inbox rows show it; TUI AI summary shows a `People:` line under the reach line.
- [ ] MCP `prs_query` rows carry `people_needed`; `priority_score` is unchanged for every PR.
- [ ] `docs/config-reference.md` documents the new section.

**Risk:** ordinary. Display only, and new config sections default to empty. The one sharp edge is the config parser: a wrong type in a new section must not reset the user's config (Step 2).

## Before you start (orientation)

- **Branch:** feature. See `../README.md` "Branching" (current `release/v*`).
- **Commands:**
  - Engine tests: `~/.claude/scripts/locked.sh . cargo test -p er-engine people_needed`
  - TUI tests: `~/.claude/scripts/locked.sh . cargo test -p er-tui people_line`
  - Desktop backend tests: `~/.claude/scripts/locked.sh . cargo test -p er-desktop people_needed`
  - MCP tests: `~/.claude/scripts/locked.sh . cargo test -p er-mcp`
  - Desktop UI tests: `~/.claude/scripts/locked.sh desktop-ui bun test src/lib/triageCard.test.ts src/lib/peopleNeeded.test.ts`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`, `~/.claude/scripts/locked.sh . just check-ui`, `~/.claude/scripts/locked.sh . just docs-check`, `~/.claude/scripts/locked.sh . just comments-check`
- **The map** (all confirmed at c96dc0f12):
  - `crates/er-engine/src/ai/triage.rs` — `TriageReview` (line 24), lenient `reach` pattern (lines 41–42, 152–161). New `needs_person` field goes here.
  - `crates/er-engine/src/ai/review.rs:163` — `RiskLevel { High, Medium, Low, Info }`; `Info` is the default and also where an unreadable word lands (`lenient_risk_level`, line 151).
  - `crates/er-engine/src/config/path_rules.rs:66` — `matching_rule`, the shared exact → glob → file-type precedence.
  - `crates/er-engine/src/config/file_kinds.rs` — the module to copy for a new per-repo config section.
  - `crates/er-engine/src/config/mod.rs:24` — `ErConfig`; lines 2 and 14 show how `file_kinds` is declared and re-exported.
  - `crates/er-engine/src/app/state/mod.rs` — `TabState.importance` / `file_kinds` (lines 868, 872), `pr_data` (887), `rules_key` (4519), `copy_repo_rules` (6288), five `TabState` struct literals (1568, 1703, 1832, 1974, 9088).
  - `crates/er-engine/src/ai/prompts.rs:1112` — `triage_lens_instructions`; `:1161` — `triage_output_section`; triage prompt tests near line 2531.
  - `crates/er-engine/src/sidecar_specs.rs` — the triage JSON schema handed to MCP agents (the `reach` block is at line 90).
  - `crates/er-engine/src/review_queue.rs:57` — `RankedPr`, built only in `score_pr` (line 255).
  - `crates/er-engine/src/sidecar_summary.rs:257` — `summarize_pr_sidecars`, the existing way to find a PR bucket from `owner`, `repo`, `number`.
  - `crates/er-mcp/src/server.rs:525` — `query_single_repo`; `:629` — `query_cross_repo`.
  - `crates/er-desktop/src/snapshot.rs:1321` — `TriageSnapshot`; `:4332` — where it is built; `:730` — `ProjectSnapshot`; `:3568` — `build_projects` (cache wrapper); `:3855` — the one `ProjectSnapshot` literal.
  - `crates/er-tui/src/ui/panel.rs:715` — where `reach_line` is pushed in `render_ai_summary`; `:1446` — `reach_line`.
  - `desktop-ui/src/lib/triageCard.ts` — `summaryPills`; `desktop-ui/src/lib/types.ts:221` — `TriageSnapshot`, `:388` — `ProjectSnapshot`.
  - `desktop-ui/src/lib/components/LeftSidebar.svelte:1100` — `prRow` snippet; `desktop-ui/src/lib/components/InboxPanel.svelte:115` — `inboxRow` snippet; `desktop-ui/src/lib/inboxCategories.ts:178` — `inboxItemProjectId`.
  - `docs/config-reference.md` — "All options" section (line 30).
- **Conventions to follow.** A triage block that may be malformed degrades instead of failing the whole file (`crates/er-engine/src/ai/triage.rs:152`):

  ```rust
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

  A per-repo table copied onto the tab (`crates/er-engine/src/app/state/mod.rs:6288`):

  ```rust
  fn copy_repo_rules(config: &ErConfig, tab: &mut TabState) {
      let repo = tab.rules_key();
      tab.importance = config.importance.repo(&repo).cloned().unwrap_or_default();
      tab.file_kinds = config.file_kinds.repo(&repo).cloned().unwrap_or_default();
  }
  ```

- **Mental model.** Triage writes `triage.json` into the view bucket; the engine
  reads it into `tab.ai.triage`. Per-repo rule tables live in the global config,
  keyed by `storage::rules_key`, and are copied onto each tab (ADR 0037). The
  desktop turns engine state into a snapshot in `crates/er-desktop/src/snapshot.rs`;
  the TUI renders straight from `TabState`. MCP ranks PRs with
  `review_queue::score_pr` and reads PR buckets with
  `storage::pr_bucket_dir(owner_repo_storage_slug(owner, repo), number)`.
  `er-mcp` links `er-engine` with `default-features = false`
  (`crates/er-mcp/Cargo.toml:15`), so anything MCP calls must live in an
  always-compiled module (`app` and `arena` are `#[cfg(feature = "ui")]` in
  `crates/er-engine/src/lib.rs`).
- **ADRs to read first.**
  - 0005 — config is global-only; no repo-local file. The new section lives in the global config.
  - 0006 — engine state is the UI contract; add explicit fields, never repurpose one.
  - 0030 — semantic theme tokens; colors come from tokens in both front ends.
  - 0036 — importance is declared config, and why the "flag-guarded tier" was cut.
  - 0037 — a tab carries the config it filters by (`copy_repo_rules`).
  - 0039 — reach is judged from engine facts; "a guess must not lower risk".

## Scope boundaries

- **In scope:** the engine function and its types; the `[people_needed.<repo>]`
  config section; the `needs_person` triage field (prompt, schema, struct,
  snapshot); `CONTEXT.md`, a new ADR, `docs/config-reference.md`; desktop triage
  card pill, sidebar "to review" PR rows, review-request inbox rows; TUI AI
  summary line; MCP `prs_query` rows.
- **OUT of scope / DO NOT TOUCH:**
  - Any effect on `priority_score`, sort order, approve or merge buttons, or GitHub. No auto-approve, no auto-merge (README "Decisions already made").
  - A Settings UI for the new section. Users edit `config.toml` by hand for now.
  - The `[importance.<repo>]` table. Do not put the new rules inside it: `ImportanceProposal::merge_into_global_config` (`crates/er-engine/src/config/mod.rs:158`) replaces a repo's whole importance table when importance is regenerated, which would erase hand-written rules.
  - The `er-queue` skill text (`skills/er-queue/SKILL.md` and its two copies under `npm/skills/`). Mention the new field in your final report as a follow-up.
  - Triage written in a **local branch view** bucket. The sidebar reads only the PR bucket (Decision log).

## Steps

### Step 1 — Name the concept and record the decision

- **Goal:** `CONTEXT.md` entry and an ADR, before any code, so names in code match.
- **Why / invariant:** `CONTEXT.md` already owns "Tier". The new word must not collide.
- **Files:** `CONTEXT.md` (add after the **Reach** entry, line 156–162); `docs/adr/<next>-people-needed-is-a-display-label.md`. At c96dc0f12 the next free number is 0044; run `ls docs/adr | tail -3` and take the next one, since other review-flow tasks also add ADRs.
- **Do this:**
  - `CONTEXT.md`:

    ```markdown
    **People needed**:
    How many people should look at a pull request before it merges: none, one,
    or two, or unknown when there is no triage to judge from. The engine works it
    out from triage, the repo's always-a-person paths and its trusted authors. A
    label only: it approves, blocks and reorders nothing.
    _Avoid_: tier, risk tier, approval level
    ```

  - ADR (short; context, decision, consequences). It must record: computed in
    the engine from triage plus declared config, deterministic, no model call;
    display only, which is how it differs from the "flag-guarded tier" and the
    auto-approve/auto-merge cut in the earlier review-triage-pipeline plan (ADR
    0036 also records the flag-tier cut); a missing signal never lowers the
    label; strict trusted-author default (Decision log); its own config section
    instead of the importance table, and why.
- **Watch out for:** `just docs-check` fails on file tables or inventory headings in instruction docs. Keep the ADR to prose.
- **Verify:** `~/.claude/scripts/locked.sh . just docs-check` → exits 0.
- **If it fails:** read the script's message; it names the broken link or heading.

### Step 2 — Config section `[people_needed.<repo>]`, test first

- **Goal:** a per-repo table holding `trusted_authors` and `always_human`.
- **Why / invariant:** `load_global_config` falls back to built-in defaults when the file does not deserialize (`crates/er-engine/src/config/mod.rs:1818–1826`: it copies the file to `config.toml.invalid` and returns `None`). A list written as a string, or a number where a string belongs, must cost that one entry, never the user's whole config.
- **Files:**
  - New `crates/er-engine/src/config/people_needed.rs`, modeled on `crates/er-engine/src/config/file_kinds.rs`.
  - `crates/er-engine/src/config/mod.rs:2` (add `mod people_needed;`), `:14` (add `pub use people_needed::{PeopleNeededConfig, PeopleNeededRepoConfig};`), `:24` (add field to `ErConfig` after `file_kinds`).
- **Do this:**
  - TOML shape (document exactly this in Step 9):

    ```toml
    [people_needed.easy-review]
    trusted_authors = ["VilfredSikker"]

    [people_needed.easy-review.always_human]
    ".github/workflows/**" = "one"
    "crates/er-engine/src/release_update.rs" = "two"
    ```

  - Types:

    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    #[serde(default)]
    pub struct PeopleNeededConfig {
        #[serde(flatten)]
        pub items: BTreeMap<String, PeopleNeededRepoConfig>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
    #[serde(default)]
    pub struct PeopleNeededRepoConfig {
        /// GitHub logins whose PRs may get the lighter labels.
        #[serde(deserialize_with = "lenient_logins")]
        pub trusted_authors: Vec<String>,
        /// Path, glob or file type → "one" | "two".
        #[serde(deserialize_with = "lenient_string_table")]
        pub always_human: BTreeMap<String, String>,
    }
    ```

    `PeopleNeededConfig::repo(&self, repo: &str) -> Option<&PeopleNeededRepoConfig>` like `FileKindsConfig::repo`.
    `ErConfig` gets `#[serde(default)] pub people_needed: PeopleNeededConfig,`.
  - `lenient_logins`: deserialize into `Option<toml::Value>`; accept an array (keep its string items, drop others) or a single string (one login); anything else gives an empty list. `lenient_string_table`: deserialize into `Option<toml::Value>`; keep entries whose value is a string; drop the rest. The engine already uses `toml::Value` and `toml::Table` directly (`crates/er-engine/src/config/mod.rs:1808`, `:1818`).
  - Tests in the new file (write them first; they fail to compile before the types exist, which counts as failing):
    - `people_needed_table_reads_authors_and_paths` — the TOML from this step parses; `repo("easy-review")` has one author and two path rules.
    - `people_needed_wrong_types_cost_only_that_entry` — `trusted_authors = "alice"` reads as `["alice"]`; `trusted_authors = 3` reads as empty; an `always_human` value `= 2` is dropped while a sibling string entry survives; and a `[display]` key in the same TOML still loads (copy `importance_skips_a_tier_it_cannot_read`, `crates/er-engine/src/config/mod.rs:3098`, which parses with `toml::from_str` into `ErConfig`).
- **Watch out for:** `path_rules::MATCH_OPTIONS` sets `require_literal_separator: true`, so `.github/workflows/*` matches only direct children; `**` is needed for nested files. A key with no `/` (e.g. `"*.lock"`) matches the basename. Say this in the docs (Step 9).
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine people_needed_` → both new tests `ok`, `test result: ok`.
- **If it fails:** if `#[serde(flatten)]` plus `deserialize_with` will not combine on a field, keep `always_human` as a nested table (it already is) and apply the lenient function there; never fall back to strict types.

### Step 3 — Triage field `needs_person`

- **Goal:** triage can say "this change needs a person whatever its risk", with a reason.
- **Why / invariant:** an absent or malformed block must mean "not stated", never "no person needed". The field is `Option`, read leniently like `reach`. A `bool` with `default = false` is forbidden: it would read silence as "no".
- **Files:** every file that carries the triage schema. At c96dc0f12, `grep -rln touch_points crates desktop-ui/src` lists exactly these, and each needs the new field:
  - `crates/er-engine/src/ai/triage.rs` — struct + lenient reader + tests.
  - `crates/er-engine/src/ai/prompts.rs` — `triage_lens_instructions` (line 1112) and `triage_output_section` (line 1161).
  - `crates/er-engine/src/sidecar_specs.rs` — triage schema, beside `reach` (line 90).
  - `crates/er-desktop/src/snapshot.rs` — `TriageSnapshot` (1321) and its builder (4332).
  - `desktop-ui/src/lib/types.ts` — `TriageSnapshot` (221).
  - `desktop-ui/src/lib/stories/fixtures.ts` — add the field to `triageBroadGuarded` (line 288) so Storybook shows it.
  - `TriageReview` has no `Default`, so the struct literals at `crates/er-engine/src/sidecar_upload.rs:565`, `crates/er-engine/src/sidecar_summary.rs:328` and `:438` gain `needs_person: None`.
- **Do this:**
  - `triage.rs`:

    ```rust
    /// Triage's answer to "does this change need a person even if its risk says not".
    #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
    pub struct TriageNeedsPerson {
        #[serde(default)]
        pub needed: bool,
        #[serde(default)]
        pub reason: String,
    }
    ```

    On `TriageReview`, after `reach`:
    `#[serde(default, deserialize_with = "lenient_needs_person")] pub needs_person: Option<TriageNeedsPerson>,`
    where `lenient_needs_person` copies `lenient_reach` but returns `Option` (`.and_then(|v| serde_json::from_value(v).ok())`, no `unwrap_or_default`). A block without a `needed` key must read as `None`: deserialize into `serde_json::Value` first and return `None` unless `needed` is a JSON bool.
  - Tests in `triage.rs` (`mod tests`, line 197): `needs_person_absent_reads_as_not_stated`, `needs_person_with_reason_reads`, `malformed_needs_person_keeps_the_rest_of_the_triage` (copy the loop in `malformed_reach_keeps_the_rest_of_the_triage`, line 230, with values `"yes"`, `3`, `{}`).
  - Prompt: in `triage_lens_instructions`, add a deliverable at the end of the numbered list, as the next free number (6 at c96dc0f12, after `priority_files`; tasks 10 and 15 also append triage deliverables, so if one of them landed first, use the number after theirs and renumber the text below to match):
    `6. \`needs_person\` — \`needed\` (true/false) and \`reason\`. Set \`needed: true\` when this particular change should have a person look at it whatever \`approx_risk\` says: it changes who can do what, touches money, credentials, personal data, release or install paths, CI, or deletes data. Say why in one sentence. Leave it out if you cannot tell.`
    In `triage_output_section`, add after the `reach` block: `"needs_person": {{ "needed": true, "reason": "Changes the updater's signature check" }}` (remember the `{{`/`}}` escaping in that `format!`).
  - Prompt test: extend `triage_and_review_prompts_carry_reach_rules` or add `triage_prompt_asks_for_needs_person` asserting `triage.contains(r#""needs_person": {"#)`.
  - Schema: `"needs_person": { "type": "object", "properties": { "needed": { "type": "boolean" }, "reason": { "type": "string" } } }`.
  - Snapshot: `TriageSnapshot` gains `pub needs_person: Option<TriageNeedsPersonSnapshot>` (`needed: bool`, `reason: String`); TS type gains `needs_person?: { needed: boolean; reason: string } | null`.
- **Watch out for:** the TUI uses the local-managed prompt `build_triage_review_prompt_local_managed` (line 1204) and the desktop uses `build_triage_review_prompt_prepared_diff` (line 1250); both call the two helpers you edit, so one edit covers both.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine triage` → all triage tests `ok`, including the three new ones and the prompt test.
- **If it fails:** a failing old triage test means the new field changed parsing of existing files. Stop and fix the reader; never edit the old test.

### Step 4 — The engine function, table-driven test first

- **Goal:** one pure function that every surface calls.
- **Why / invariant:** deterministic, no I/O, no model call; a missing signal never lowers the label; lives in an always-compiled module so er-mcp can call it.
- **Files:** new `crates/er-engine/src/ai/people_needed.rs`; `crates/er-engine/src/ai/mod.rs` (add `pub mod people_needed;` beside `pub mod triage;` at line 14, and `pub use people_needed::*;` beside line 30).
- **Do this:**
  - Types:

    ```rust
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
    pub enum PeopleNeeded {
        #[serde(rename = "none")]
        NoPerson,
        #[serde(rename = "one")]
        OnePerson,
        #[serde(rename = "two")]
        TwoPeople,
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    pub struct PeopleNeededLabel {
        /// `None` = unknown: the triage gives no risk the label can stand on.
        pub level: Option<PeopleNeeded>,
        /// Plain-language reasons, in the order the rules applied.
        pub reasons: Vec<String>,
        /// False when the changed paths were not available to check.
        pub paths_checked: bool,
        /// Whether the triage was produced for the diff being shown.
        pub fresh: bool,
    }

    pub struct PeopleNeededInputs<'a> {
        pub triage: &'a TriageReview,
        pub fresh: bool,
        /// `None` when the caller has no diff loaded.
        pub changed_paths: Option<&'a [String]>,
        /// GitHub login of the PR author; `None` when unknown.
        pub author: Option<&'a str>,
        pub rules: Option<&'a crate::config::PeopleNeededRepoConfig>,
    }

    pub fn people_needed(inputs: &PeopleNeededInputs<'_>) -> PeopleNeededLabel
    ```

    `path_rules::matching_rule` is `pub(crate)` in `crate::config::path_rules`, and `path_rules` is a private module of `config` (`mod path_rules;`, line 4). Add a method on `PeopleNeededRepoConfig` in `config/people_needed.rs` that wraps it, so `ai/` does not reach into a private module:
    `pub fn always_human_rule(&self, path: &str) -> Option<(&str, PeopleNeeded)>` using parse `|v| Some(if v.trim().eq_ignore_ascii_case("two") { TwoPeople } else { OnePerson })`. The parse never returns `None`: a misspelled value still counts as one person, because `matching_rule` treats an unparsed winner as "no rule" and a typo must not remove a floor.
  - **Decision table** (apply in this order):

    | # | Rule | Result |
    |---|---|---|
    | 1 | No triage | no label: callers return `None` and never call `people_needed` (`TabState::people_needed` in Step 5, `people_needed_for_pr` in Step 6) |
    | 2 | Base from `approx_risk`: `High` → two; `Medium` → one; `Low` with reach `Isolated` or `Contained` → none; `Low` with reach `Broad` or `Unknown` → one; `Info` → unknown | reason names the risk and reach |
    | 3 | `needs_person` is `Some` with `needed: true` | at least one; reason is the triage's reason, or "triage says a person should look" when empty |
    | 4 | Any changed path matches `always_human` | at least the rule's value; reason `"<path> matches <key>"` for the highest match (first in path order on ties) |
    | 5 | `changed_paths` is `None` and the label so far is none | raise to one; reason "changed paths not checked"; `paths_checked: false` |
    | 6 | Author not in `trusted_authors` (case-insensitive), or author unknown | one step up (none → one, one → two, two stays two); reason "author not on the trusted list" (add "(no list declared)" when the list is empty). Skipped when the label is still unknown |

    "At least" on an unknown label sets it to that value (a floor gives a known lower bound). Rule 6 runs after the floors.
  - Table-driven test `people_needed_follows_the_decision_table`: one row per table line plus these combinations, each asserting `level` and that `reasons` is non-empty:
    - high risk, trusted author → two
    - low + isolated, trusted author, paths checked, nothing matches → none
    - low + isolated, untrusted author → one
    - low + unknown reach → one
    - info risk, no floors → unknown
    - info risk, a `.github/workflows/ci.yml` change under `".github/workflows/**" = "two"` → two
    - low + contained, trusted, `changed_paths: None` → one with `paths_checked: false`
    - low + isolated, trusted, `needs_person` needed with empty reason → one, reason "triage says a person should look"
    - medium, untrusted, empty trusted list → two
    - a rule value `"tow"` → one (typo keeps the floor)
- **Watch out for:** `RiskLevel::Info` is also what an unreadable word becomes (`crates/er-engine/src/ai/review.rs:167–170`), which is why `Info` maps to unknown and never to none.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine people_needed` → the table test plus Step 2's tests `ok`. Then plant a break and revert it in one command to prove the test bites, e.g. `sed -i '' 's/eq_ignore_ascii_case("two")/eq_ignore_ascii_case("zzz")/' crates/er-engine/src/config/people_needed.rs && ~/.claude/scripts/locked.sh . cargo test -p er-engine people_needed; sed -i '' 's/eq_ignore_ascii_case("zzz")/eq_ignore_ascii_case("two")/' crates/er-engine/src/config/people_needed.rs` → expect the "two" rows to fail, then `grep -c 'eq_ignore_ascii_case("two")' crates/er-engine/src/config/people_needed.rs` → `1` (file restored). The `sed -i ''` form is macOS sed; on GNU sed drop the `''`.
- **If it fails:** fix the function to the table. If a table row seems wrong for the product, stop and ask; do not change the table.

### Step 5 — Tab method and rule copy (both front ends' source)

- **Goal:** `TabState` carries the repo's `PeopleNeededRepoConfig` and exposes the label.
- **Why / invariant:** ADR 0037: a tab carries the config it resolves against, copied in `copy_repo_rules` and refreshed by `sync_repo_rules_to_tabs`. Computing per render is acceptable here: `TabState::visible_files` (line 4548) already resolves importance rules per file on each call.
- **Files:** `crates/er-engine/src/app/state/mod.rs` — field after `file_kinds` (line 872); add `people_rules: PeopleNeededRepoConfig::default(),` to the five literals (1568, 1703, 1832, 1974, 9088); one line in `copy_repo_rules` (6288); new method near `repo_rules` (4532).
- **Do this:**

  ```rust
  // in copy_repo_rules
  tab.people_rules = config.people_needed.repo(&repo).cloned().unwrap_or_default();

  /// The People needed label for this tab's triage and diff.
  pub fn people_needed(&self) -> Option<crate::ai::PeopleNeededLabel> {
      let triage = self.ai.triage.as_ref()?;
      let fresh = crate::ai::triage_is_fresh(triage, &self.branch_diff_hash);
      let paths: Vec<String> = self.files.iter().map(|f| f.path.clone()).collect();
      Some(crate::ai::people_needed(&crate::ai::PeopleNeededInputs {
          triage,
          fresh,
          changed_paths: Some(&paths),
          author: self.pr_data.as_ref().map(|p| p.author.as_str()),
          rules: Some(&self.people_rules),
      }))
  }
  ```

  Test next to `copy_repo_rules_hands_a_tab_only_its_repos_file_kinds` (line 9849), named `copy_repo_rules_hands_a_tab_its_people_needed_rules`, same shape: insert a table for `my-service`, assert it lands on the tab, then assert another repo gets the default.
- **Watch out for:** use `self.files`, not `active_diff_files()`: in History mode the latter returns one commit's files, which is not what triage judged.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine copy_repo_rules` → three tests `ok` (two existing, one new).
- **If it fails:** a missing literal shows up as "missing field `people_rules`"; add it there.

### Step 6 — PR-bucket reader with a small cache (sidebar and MCP source)

- **Goal:** compute the label for a PR that is not open in a tab, from its PR bucket.
- **Why / invariant:** the desktop sidebar is rebuilt on every snapshot behind `ProjectsCacheKey` (`crates/er-desktop/src/snapshot.rs:3596`), whose inputs do not change when a triage finishes or config changes. So the label must be computed outside that cache, and re-reading `triage.json` plus the raw diff on every snapshot would cost too much. Cache by file modification time.
- **Files:** `crates/er-engine/src/ai/people_needed.rs` (same file as Step 4).
- **Do this:**
  - `pub fn people_needed_for_pr(owner: &str, repo: &str, number: u64, author: Option<&str>, config: &crate::config::ErConfig) -> Option<PeopleNeededLabel>`:
    - bucket: `crate::storage::pr_bucket_dir(&crate::github::owner_repo_storage_slug(owner, repo), number)` (same as `summarize_pr_sidecars`, `crates/er-engine/src/sidecar_summary.rs:257–260`).
    - return `None` when `triage.json` does not exist (no label at all in lists; "not triaged" is noise on every row).
    - triage: `crate::ai::load_triage_review(bucket_str)`.
    - changed paths: only when `bucket/.diff-tmp.sha256` trimmed equals `triage.diff_hash`, read `bucket/diff-tmp` and take `crate::git::parse_diff_headers(&raw)` paths; otherwise `None` (rule 5 then applies).
    - rules: `config.people_needed.repo(&crate::storage::rules_key("", Some(&format!("{owner}/{repo}"))))` (the remote-only key pattern at `crates/er-engine/src/sidecar_upload.rs:229`).
    - `fresh: true` (a bucket has no current diff to compare with; see Decision log).
  - Cache: a `static` `Mutex<HashMap<PathBuf, (Option<SystemTime>, Option<SystemTime>, TriageReview, Option<Vec<String>>)>>` keyed by bucket, holding the modification times of `triage.json` and `diff-tmp`; reuse the entry when both match. Clear it when it passes 256 entries (the same clear-and-refill approach as `PATTERN_CACHE_CAP` in `crates/er-engine/src/config/path_rules.rs:26`). Config is applied after the cache on every call, so a config edit shows on the next snapshot.
  - Test `people_needed_for_pr_reads_the_pr_bucket`: set `ER_STORAGE_ROOT` to a temp dir while holding `crate::storage::STORAGE_TEST_ENV_LOCK` (`crates/er-engine/src/storage.rs:217`); copy the `with_storage_root` helper at `crates/er-engine/src/sidecar_upload.rs:416`. Write a `triage.json` with `approx_risk: "low"`, reach `isolated`, a `diff-tmp` touching `.github/workflows/ci.yml`, and the matching `.diff-tmp.sha256`; with a config whose `always_human` has `".github/workflows/**" = "one"` and the author trusted, expect `OnePerson`. Without the sha marker, expect `OnePerson` with `paths_checked: false`.
- **Watch out for:** `git` module exports `parse_diff_headers` (`crates/er-engine/src/git/mod.rs:27–31`). Do not use `parse_diff`; it builds every line.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine people_needed_for_pr` → `ok`.
- **If it fails:** if tests that set `ER_STORAGE_ROOT` interfere with each other, use the serialisation helper the existing storage tests use; do not drop the test.

### Step 7 — MCP `prs_query`

- **Goal:** each ranked row carries `people_needed`.
- **Why / invariant:** display only: `priority_score` and sort order must be identical before and after.
- **Files:** `crates/er-engine/src/review_queue.rs:57` (`RankedPr`) and `:255` (its one literal); `crates/er-mcp/src/server.rs:525` (`query_single_repo`), `:629` (`query_cross_repo`).
- **Do this:**
  - `RankedPr` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub people_needed: Option<crate::ai::PeopleNeededLabel>,` and `score_pr` sets `people_needed: None`. `RankedPr` derives `Deserialize`, which is why Step 4's label type derives it too.
  - In `query_single_repo`, change `let ranked: Vec<RankedPr>` to `let mut ranked`, load `er_engine::config::load_global_config()` once, then for each row set `row.people_needed = people_needed_for_pr(&owner, &name, row.pr.number, Some(&row.pr.author), &config)`. Same in `query_cross_repo` after `tagged.truncate(limit)`, over `tagged[i].ranked`. Do it after truncation so at most `limit` buckets are read.
  - Engine test in `review_queue.rs` tests: `score_pr_leaves_people_needed_unset` (asserts `None` and that the score of the existing `tiny_ready_pr_outranks_huge_one` fixture is unchanged).
- **Watch out for:** `load_global_config` runs a migration on first use; call it once per query.
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-engine review_queue` → all `ok`; `~/.claude/scripts/locked.sh . cargo test -p er-mcp` → `test result: ok`.
- **If it fails:** a serde error on `RankedPr` means a missing derive on the label types.

### Step 8 — Front ends

- **Goal:** show the label in the TUI, the desktop triage card, the sidebar and the inbox.
- **Why / invariant:** same word and same values everywhere: "no person", "1 person", "2 people", "unknown". Colors from theme tokens only (ADR 0030): none → success, one → neutral, two → warning, unknown → neutral.
- **Files and Do this:**
  - **TUI** `crates/er-tui/src/ui/panel.rs`: add `fn people_line(label: Option<&er_engine::ai::PeopleNeededLabel>) -> Option<Line<'static>>` beside `reach_line` (line 1446): ` People: 2 people · <first reason>` with `styles::GREEN()` / `styles::TEXT()` / `styles::YELLOW()` / `styles::DIM()` (unknown) for the value; `None` when `label` is `None` (no triage). Push it right after the reach line (line 715): `if let Some(line) = people_line(tab.people_needed().as_ref()) { lines.push(line); }`. Tests beside `reach_line_hidden_for_triage_without_reach` (line 1481): `people_line_hidden_without_triage`, `people_line_names_the_first_reason`.
  - **Desktop triage card:** `TriageSnapshot` (`crates/er-desktop/src/snapshot.rs:1321`) gains `pub people_needed: Option<PeopleNeededSnapshot>` with `level: String` (`none`/`one`/`two`/`unknown`), `reasons: Vec<String>`, `paths_checked: bool`, `fresh: bool`; fill it from `tab.people_needed().map(…)` in the builder at line 4332 (inside `build_ai_snapshot`, line 4007). Add a test in `snapshot.rs` named `triage_snapshot_carries_people_needed`, shaped like `ai_snapshot_marks_github_outdated_comment_stale_for_ui` (line 4985): `TabState::new_for_test(vec![])`, set `tab.ai.triage` to a high-risk triage, call `build_ai_snapshot(&tab, None)`, assert `level == "two"`. TS: add the type to `desktop-ui/src/lib/types.ts`. New `desktop-ui/src/lib/peopleNeeded.ts` with `peopleNeededLabel(level)` and `peopleNeededTone(level): PillTone`, plus `peopleNeeded.test.ts`. In `desktop-ui/src/lib/triageCard.ts` `summaryPills`, push the people pill **first** when `triage.people_needed` is present, title = reasons joined with " · ". Existing `summaryPills` tests build triage without `people_needed`, so they must pass unchanged; add `it("puts the people-needed pill first when present")`. Give `triageBroadGuarded` in `desktop-ui/src/lib/stories/fixtures.ts` (line 288) a `people_needed` value (`level: "two"`, one reason) so Storybook shows the pill.
  - **Sidebar:** `ProjectSnapshot` (`snapshot.rs:730`) gains `#[serde(default)] pub people_needed: BTreeMap<u64, PeopleNeededSnapshot>`, set to empty in the literal at line 3855. In `build_projects` (line 3568), after the cache lookup or build, fill it for each project's `prs_to_review` by calling `people_needed_for_pr`, splitting the project's `remote` into owner and repo with `remote.split_once('/')` after `normalize_remote_slug` (the same split `crates/er-desktop/src/auto_triage.rs:85` uses before it resolves a PR bucket; that file's `resolve_er_dir`, line 91, lands in the same directory as `owner_repo_storage_slug`). Pass `&app.config` into `build_projects`; its caller is `build_snapshot_inner` (line 2378), which has `app`. Do not compute inside `build_projects_from_file`: its result is cached by `ProjectsCacheKey`, which does not change when a triage lands. TS: `ProjectSnapshot.people_needed?: Record<string, PeopleNeededSnapshot>`. In `LeftSidebar.svelte` `prRow` (line 1100), after the title span, show a small text label (e.g. `2p`, `1p`, `0p`) with the full text and reasons in `title`, only when an entry exists.
  - **Inbox:** in `InboxPanel.svelte` `inboxRow` (line 115), for items whose `category === "review_requested"` and whose `target.pr_number` is set, look up `projects.find(p => p.id === inboxItemProjectId(item, projects))?.people_needed?.[String(item.target.pr_number)]` and show the same small label. Pure lookup; no new backend state.
- **Watch out for:** `PeopleNeeded`'s serde names are `none`/`one`/`two`; the snapshot's `level` adds `unknown` for `level: None`. Keep that mapping in one Rust helper used by both snapshot fields.
- **Verify:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-tui people_line` → 2 `ok`.
  - `~/.claude/scripts/locked.sh . cargo test -p er-desktop people_needed` → `ok`.
  - `~/.claude/scripts/locked.sh desktop-ui bun test src/lib/triageCard.test.ts src/lib/peopleNeeded.test.ts` → all pass, 0 fail.
  - Visual: `cd desktop-ui && bun run storybook` (port 6006, script in `desktop-ui/package.json`; it may need to run outside the sandbox) and confirm the triage card story built on `triageBroadGuarded` shows the people pill first.
- **If it fails:** if `svelte-check` flags the optional fields, keep them optional in TS (fixtures predate them).

### Step 9 — Config reference

- **Goal:** document the section.
- **Files:** `docs/config-reference.md`, under "All options", after `### [packages]` (line 99).
- **Do this:** a `### [people_needed.<repo>]` subsection: what it is for (one sentence), the TOML from Step 2, the glob rule (`*` stays inside one directory, `**` crosses them, a key without `/` matches the file name), that values other than `two` count as one, that an empty or missing `trusted_authors` means nobody gets the lighter labels, that `<repo>` is the same key `[importance.<repo>]` uses, and that the label is display only. Link the ADR from Step 1.
- **Verify:** `~/.claude/scripts/locked.sh . just docs-check` → exits 0.

## Gotchas & landmines

- **Config parse failure resets everything.** `parse_config_from_path` (`crates/er-engine/src/config/mod.rs:1806`) returns `None` on any type error, and `load_global_config` then uses defaults. Every new field must be lenient (Step 2).
- **Importance regeneration replaces its table.** Keep the new rules out of `[importance.<repo>]`.
- **`Info` means "no signal" as well as "info".** Never let it yield "no person".
- **Projects cache.** Anything computed inside `build_projects_from_file` goes stale until the PR list refreshes.
- **`er-mcp` builds the engine without default features.** Keep the function out of `app/`.
- **Other tasks edit the same triage files.** Task 07 Step 9 changes the triage card's follow-up button (07 leaves the triage prompt and schema alone). Task 10 adds a `sensitive` block and task 15 adds `intent` and `pattern_setting` blocks to `TriageReview`, the triage prompt, the triage schema and `TriageSnapshot`, all after `reach` like `needs_person`. The fields do not collide. If any of them has landed, merge around its changes; do not revert them.
- **No task numbers in code comments** (`just comments-check`).
- **An older `er` binary erases the section.** It loads the config without the new field and writes back only what it knows at its next settings save. Mention this in the release notes for the release that ships it.

## Decision log (forks pre-answered)

- **Where the label is computed:** in the engine, `crates/er-engine/src/ai/people_needed.rs`, deterministic from triage plus declared config. Front ends only format it. This keeps TUI, desktop and MCP in agreement (ADR 0006) and works in the headless build.
- **Name:** **People needed**, because **Tier** is taken.
- **Config home:** its own `[people_needed.<repo>]` section, keyed like importance, with `always_human` nested. Not inside `[importance.<repo>]` (regeneration erases it) and not a repo file (ADR 0005).
- **Trusted-author default is strict:** an empty or missing list means nobody is trusted, so the best label for an unconfigured repo is "1 person". This follows the blog's "start strict and loosen with evidence". If the owner wants the opposite, it is a one-line change in rule 6.
- **Unknown author** (local branch view with no PR data) counts as untrusted.
- **A typo in an `always_human` value** counts as one person.
- **Sidebar freshness:** `triage.json` records a diff hash, not a head SHA, so the sidebar cannot tell whether triage is current. The label shows anyway; the tab view marks staleness through `fresh`. Do not add a head-SHA field to triage in this task.
- **Sidebar shows only PR-bucket triage.** Triage run in a local branch view lives in the branch bucket and is not read for the sidebar row.
- **Which sidebar rows:** `prs_to_review` only (the reviewer's queue). `my_prs` and saved PRs are out of scope.
- **Inbox:** only `review_requested` items, as a lookup into the sidebar data.
- **No triage in a list:** no label. **No triage on the triage card:** the card does not render, so nothing to do.
- **If you encounter** an existing helper that already splits `owner/repo` in er-desktop: use it.
- **10's `sensitive` block is not read here (consistency pass, 2026-10-17).** Task 10 adds `TriageReview.sensitive` (evidenced reasons that a change touches auth, credentials, personal data, payments). It decides whether the security lens runs; `needs_person` decides this label. Do not fold `sensitive` into the Step 4 decision table, which is fixed. Whether a flagged `sensitive` block should set a floor of one person is the owner's call, and a change to the table.
- **Readers of this label:** task 15's decision-point model reads `PeopleNeededLabel` (via `TabState::people_needed()`) and `PeopleNeededRepoConfig` (`trusted_authors`, `always_human_rule`). Keep those names, or record renames under `## Comments`.
- **The code field `PeopleNeededLabel.level`** keeps that name in code. In prose, say "label" or "People needed"; `CONTEXT.md` lists "level" under *Avoid*.

## Full verification

1. `~/.claude/scripts/locked.sh . cargo test -p er-engine people_needed` → `ok`.
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine triage` → `ok`.
3. `~/.claude/scripts/locked.sh . cargo test -p er-engine copy_repo_rules` → `ok`.
4. `~/.claude/scripts/locked.sh . cargo test -p er-engine review_queue` → `ok`.
5. `~/.claude/scripts/locked.sh . cargo test -p er-tui people_line` → `ok`.
6. `~/.claude/scripts/locked.sh . cargo test -p er-desktop people_needed` → `ok`.
7. `~/.claude/scripts/locked.sh . cargo test -p er-mcp` → `test result: ok`.
8. `~/.claude/scripts/locked.sh desktop-ui bun test src/lib/triageCard.test.ts src/lib/peopleNeeded.test.ts` → 0 fail.
9. `~/.claude/scripts/locked.sh . just test` → `test result: ok` for er-engine and er-tui.
10. `~/.claude/scripts/locked.sh . just clippy` → exits 0. `~/.claude/scripts/locked.sh . just check-ui` and `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
11. `~/.claude/scripts/locked.sh . just docs-check` and `~/.claude/scripts/locked.sh . just comments-check` → exit 0.
12. Manual: add the Step 2 TOML for a repo you have open, run triage on a PR in the desktop, and confirm the triage card pill, the sidebar row label and (if you have a review-request item) the inbox label agree. In the TUI, open the AI panel and confirm the `People:` line.

## Rollback

All changes are additive. Revert the commits. A `[people_needed.*]` section left in a user's config after rollback still loads: nothing under `crates/er-engine/src/config` uses `deny_unknown_fields` (checked at c96dc0f12), so serde skips it. It is dropped at the next settings save, because `save_config` writes only what `ErConfig` holds.

## When to STOP and ask

- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- A row of the Step 4 decision table looks wrong for the product. Ask; do not change it.
- Snapshot build time visibly grows (the sidebar or inbox lags) after Step 8.
- Anyone asks for the label to change sort order, gate approve, or post to GitHub.

## Comments
