# 13 — More evidence: CI failure details and a PR template check

Status: ready-for-agent
Idea: 8 in the original idea list (summarised in ../README.md)
Depends on: —
Impact: M · Complexity: M · Size: M
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — When a CI check fails, er shows its name, a red dot and a link out to GitHub. To learn why it failed you leave er and dig through the CI site. Separately, er shows the PR description but never notices when it skips the parts a reviewer needs: how it was tested, what the risk is, and how to undo it.
- **Why it matters** — Reading the evidence is one of the few decisions left to a person once machines do most of the review. If the evidence is a click and a context switch away, people approve without reading it.
- **The fix** — (A) er checks the PR description for Testing, Risk and How to revert sections and says which are missing. (B) For a failing GitHub Actions check, a "Show failure" toggle fetches the failure's annotations (file, line, message) and the tail of the failed step's log, on click only.
- **TL;DR** — Put the failing check's error and the missing PR-description sections in front of the reviewer, inside er.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> The PR template asks how the change was tested and how to undo it. Claude can fill that in, but the person who asked for the change signs off on it.

> **Reading the evidence.** Test output, the command that ran and what it returned, a screenshot, the reviewer's findings. If you can't say what the checks would miss, ask for a real review rather than a quick approval.

And from its checklist: "Require CI, and add "Testing", "Risk" and "How to revert" to your PR template."

## Mission & definition of done
Part A: an engine function reports which of Testing / Risk / How to revert a PR body lacks (missing or left empty). The desktop GitHub card and the TUI PR overview show it as a one-line, informational note. No new `gh` call. Part B: the desktop expanded checks list gets a "Show failure" toggle on each failing check. Clicking it runs one new Tauri command that, for a GitHub Actions job, fetches annotations and the failed-step log tail through `gh`, caches the result per job, and shows it; for any other CI it says the details live on the provider and keeps the link. Nothing new runs on a timer.

- [ ] `er_engine::pr_evidence::pr_body_gaps(body: &str) -> Vec<&'static str>` with unit tests for headings, bold-line headings, synonyms, empty sections, HTML-comment-only sections, and an empty body.
- [ ] `GithubStatusSnapshot` carries `pr_body_gaps: Vec<String>` (serde default), filled in `fetch_github_status`; BranchCard shows the note; the TUI PR overview shows the note.
- [ ] `er_engine::pr_evidence::parse_actions_job_link`, `parse_check_annotations`, `log_tail` have unit tests; `fetch_check_evidence` caches per (owner, repo, job id).
- [ ] Tauri command `fetch_check_evidence` is registered and only reached from a click.
- [ ] `CheckEvidence.svelte` renders annotations and the log tail as plain text.
- [ ] `just clippy`, `just lint-ui`, `just comments-check` exit 0; `just test`, `just test-desktop`, `just test-ui` pass.

**Risk:** ordinary. Read-only GitHub calls on click; a new snapshot field with a default.

## Before you start (orientation)
- **Branch:** feature. Use the current release branch; see ../README.md "Branching".
- **Commands:**
  - Engine tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pr_evidence`
  - TUI tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui`
  - Desktop backend tests: `~/.claude/scripts/locked.sh . cargo test -p er-desktop <filter>`
  - Frontend: `~/.claude/scripts/locked.sh desktop-ui bun run test`, `~/.claude/scripts/locked.sh desktop-ui bun run check`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`, `~/.claude/scripts/locked.sh . just comments-check`
- **The map**
  - New: `crates/er-engine/src/pr_evidence.rs` (pure parsers + the two `gh` calls), declared in `crates/er-engine/src/lib.rs` beside `pub mod pr_resolve;` (line 32). Always compiled; it depends only on `github` (always compiled, `lib.rs:27`).
  - `crates/er-engine/src/github.rs` — `CheckRun` (2586), `parse_pr_checks` (2665, maps `link` → `url`), `check_run_is_failing` (3109), `summarize_checks_state` (3125), `gh_pr_checks_remote` (2793, runs `gh pr checks --json name,state,bucket,link`), `run_parallel` (1963, `pub(crate)`), cache pattern `PrCommentsCache` (2000-2050).
  - `crates/er-desktop/src/snapshot.rs` — `CheckSummary` (1350), `GithubStatusSnapshot` (1374-1427).
  - `crates/er-desktop/src/commands.rs` — `fetch_github_status` (697-806) builds the snapshot; `body: overview.body` at 781.
  - Snapshot literals that must gain the new field: `crates/er-desktop/src/gh_status_cache.rs:196`, `crates/er-desktop/src/gh_pr_actions.rs:304` and `:340`. Find any others with `grep -rn "GithubStatusSnapshot {" crates`.
  - New: `crates/er-desktop/src/check_evidence.rs` (the Tauri command), declared in `crates/er-desktop/src/lib.rs` (module list, `pub mod gh_pr_actions;` at line 10), registered in `crates/er-desktop/src/main.rs` `generate_handler!` (1895; `gh_pr_actions::run_github_pr_action` at 1948) and added to the `use er_desktop::{…}` list at `main.rs:12-15`.
  - `crates/er-desktop/src/gh_pr_actions.rs:45` — `pub struct PrTarget { owner, repo, number }`, reused as the command's argument.
  - `desktop-ui/src/lib/types.ts` — `CheckSummary` (448), `GithubStatusSnapshot` (515).
  - `desktop-ui/src/lib/components/BranchCard.svelte` — expanded checks list (516-543), description block (626-645).
  - New: `desktop-ui/src/lib/components/CheckEvidence.svelte` and `desktop-ui/src/lib/components/CheckEvidence.test.ts`.
  - `desktop-ui/src/lib/checkCounts.ts` — the shared pass/fail spelling rule (`FAILURE` or `fail`).
  - `crates/er-tui/src/ui/panel.rs` — `render_pr_overview` (948); Description block (1058-1079); CI Checks block (1081-1116).
- **Conventions to follow**
  - Pure parser plus thin `gh` wrapper, like `gh_pr_checks_remote` / `parse_pr_checks` (`github.rs:2793-2810`, `2665-2700`):

    ```rust
    let output = Command::new("gh")
        .args([ "pr", "checks", &number.to_string(), "--repo", &repo_slug, "--json", "name,state,bucket,link" ])
        .output_timed(crate::proc::GH_TIMEOUT)
        .context("Failed to run gh pr checks")?;
    ```
  - Tauri commands that shell out run through `run_blocking` (ADR 0015), e.g. `crates/er-desktop/src/gh_pr_actions.rs:60-67`:

    ```rust
    #[tauri::command]
    pub async fn run_github_pr_action(pr: PrTarget, action: PrActionRequest, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
        let state = state.inner().clone();
        run_blocking(move || run_github_pr_action_blocking(&pr, action, &state)).await
    }
    ```
  - A command may return a typed payload instead of a snapshot: `request_file_preview` (`crates/er-desktop/src/commands.rs:1010`) returns `Result<FilePreviewResponse, String>`.
  - Component tests read the component's source (`desktop-ui/src/lib/components/PrMergeBox.test.ts:1-8`); pure helpers get ordinary unit tests (`desktop-ui/src/lib/checkCounts.test.ts`).
  - TUI colours come from `styles::` accessors only (`crates/er-tui/src/ui/CLAUDE.md`, "Color"); `styles::YELLOW()` and `styles::MUTED()` exist (`crates/er-tui/src/ui/styles.rs:30`, `:47`).
- **Mental model** — The desktop's 30-second status loop calls `fetch_github_status` (`commands.rs:697`), which runs `gh pr view --json …` and `gh pr checks --json name,state,bucket,link` in parallel and stores a `GithubStatusSnapshot` in `gh_status_cache`. The PR body arrives in that same `gh pr view` call, so a body check costs nothing. Check details (annotations, logs) are not in `gh pr checks`; they need one REST call and one `gh run view` per job, and there is no run or job id in the JSON, only the `link` URL. The rate-limit audit (`internal-docs/gh-rate-limit-slimming.md`) shows the status loop is already the largest source of `gh` calls, so anything added there multiplies by every open tab and every 30 seconds. The TUI shows the PR body and checks in `render_pr_overview` from `tab.pr_data` (`PrOverviewData`), fetched by `gh_pr_overview`.
- **ADRs to read first**
  - 0026 — `gh` CLI only; rate limits arrive opaquely, so call volume is er's job.
  - 0015 — commands that shell out run off the main thread.
  - 0006 — engine first; the template rule lives in the engine so both front ends use it.
  - 0005 — global-only config; the section names are not configurable in this task (see Decision log).

## Scope boundaries
- **In scope:** Part A (body gaps, engine + desktop + TUI display); Part B (failing-check evidence, engine + desktop only).
- **OUT of scope / DO NOT TOUCH:**
  - Anything on the 30-second status loop beyond adding the body-gap field, which reuses data already fetched. No new `gh` call in `fetch_github_status`, `main.rs` loops, or `kick_active_gh_status`.
  - TUI CI logs (the TUI has no on-demand fetch surface for checks; a later task can add one).
  - Attaching the writing agent's own command output (the commands it ran and what they returned) as evidence. The original idea 8 mentions it; it belongs with live review, task 14.
  - An evidence card that summarizes what the review covered (task 15).
  - Making section names configurable, and blocking merge or approval on missing sections.
  - Jumping from an annotation into the diff.
  - The fallback check rows in BranchCard (`checks_status` rows, 579-620).

## Steps

### Step 1 — Body gap check in the engine, test first
- **Goal:** one pure function says which template sections a PR body lacks.
- **Why / invariant:** informational only. It never blocks anything, and an unknown body layout must lean towards "missing" so the person reads the body.
- **Files:** new `crates/er-engine/src/pr_evidence.rs`; `crates/er-engine/src/lib.rs:32`.
- **Do this:**
  1. Create the module with a test module first:

     ```rust
     #[cfg(test)]
     mod tests {
         use super::*;

         #[test]
         fn body_with_all_sections_has_no_gaps() {
             let body = "## Summary\nx\n\n## Testing\nran cargo test\n\n## Risk\nlow\n\n## How to revert\nrevert the commit\n";
             assert!(pr_body_gaps(body).is_empty());
         }

         #[test]
         fn empty_body_lacks_everything() {
             assert_eq!(pr_body_gaps(""), vec!["Testing", "Risk", "How to revert"]);
         }

         #[test]
         fn bold_lines_and_synonyms_count_as_headings() {
             let body = "**Test plan**\nunit tests\n\n**Risks:**\nnone\n\n**Rollback**\nflip the flag\n";
             assert!(pr_body_gaps(body).is_empty());
         }

         #[test]
         fn section_with_only_a_template_comment_is_a_gap() {
             let body = "## Testing\n<!-- how did you test this? -->\n\n## Risk\nlow\n\n## How to revert\n\n";
             assert_eq!(pr_body_gaps(body), vec!["Testing", "How to revert"]);
         }

         #[test]
         fn the_word_in_prose_is_not_a_heading() {
             let body = "This PR adds testing helpers and reduces risk.\n";
             assert_eq!(pr_body_gaps(body), vec!["Testing", "Risk", "How to revert"]);
         }
     }
     ```

  2. Add `pub mod pr_evidence;` to `lib.rs` and run `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pr_evidence` → compile failure (red).
  3. Implement `pub fn pr_body_gaps(body: &str) -> Vec<&'static str>`:
     - Remove every `<!-- … -->` span first, including spans across lines.
     - A heading line is a Markdown ATX heading (1–6 `#` then a space) or a line whose whole trimmed text is wrapped in `**…**` or `__…__`, with an optional trailing `:` inside or outside the markers.
     - Lowercase the heading text. Testing matches `test` or `verif`; Risk matches `risk`; How to revert matches `revert`, `rollback`, `roll back` or `undo`.
     - A section's content runs to the next heading line. A section whose content is only whitespace is a gap. Compute that as one `let filled = !content.trim().is_empty();` line, so the planted break in this step's Verify can target it.
     - Return the gaps in the fixed order Testing, Risk, How to revert. A label is a gap when no matching heading has non-empty content.
- **Watch out for:** a heading can match two sections ("Testing and rollback"); count it for both.
- **Verify:**
  - `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pr_evidence` → `5 passed`.
  - Planted break, applied and reverted in one command (commit this step first, since `git checkout --` drops uncommitted edits): `sed -i '' 's/let filled = !content.trim().is_empty();/let filled = true;/' crates/er-engine/src/pr_evidence.rs && ~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine section_with_only_a_template_comment_is_a_gap; git checkout -- crates/er-engine/src/pr_evidence.rs` → expect the test to FAIL; afterwards `git diff --quiet crates/er-engine/src/pr_evidence.rs` exits 0.
- **If it fails:** fix the function; the fixtures express the rule.

### Step 2 — Show the gaps in the desktop and the TUI
- **Goal:** the note appears where the PR description is shown.
- **Why / invariant:** no new `gh` call: the body is already in the status bundle (`commands.rs:781`) and in `PrOverviewData.body`.
- **Files:**
  - `crates/er-desktop/src/snapshot.rs:1374` — add to `GithubStatusSnapshot`:

    ```rust
    /// Template sections (Testing, Risk, How to revert) the PR body lacks or
    /// left empty. Informational; nothing is blocked on it.
    #[serde(default)]
    pub pr_body_gaps: Vec<String>,
    ```
  - `crates/er-desktop/src/commands.rs:772-806` — compute `let pr_body_gaps = er_engine::pr_evidence::pr_body_gaps(&overview.body).into_iter().map(String::from).collect();` **before** `body: overview.body` moves the string, and set the field.
  - Every other `GithubStatusSnapshot { … }` literal (listed under "The map"): add `pr_body_gaps: vec![],`.
  - `desktop-ui/src/lib/types.ts:515` — add `pr_body_gaps?: string[];` with a one-line doc comment.
  - `desktop-ui/src/lib/components/BranchCard.svelte:626` — directly above the `(e) Description block` comment, add:

    ```svelte
    {#if github && (github.pr_body_gaps?.length ?? 0) > 0}
      <div class="text-[11px] text-risk-med" data-testid="pr-body-gaps">
        PR description has no {github.pr_body_gaps!.join(", ")} section
      </div>
    {/if}
    ```
    `text-risk-med` is the merge box's warn tone (`desktop-ui/src/lib/components/PrMergeBox.svelte:125`) and is already used in BranchCard's check row.
  - `crates/er-tui/src/ui/panel.rs` — in `render_pr_overview`, right after the Description block (ends 1079, before `// CI checks` at 1081), add one line when `er_engine::pr_evidence::pr_body_gaps(&pr.body)` is non-empty: ` Missing: Testing, How to revert` in `styles::YELLOW()`, followed by an empty line.
- **Watch out for:** the desktop persists the status cache to disk (`gh_status_cache.rs:111`); `#[serde(default)]` lets an older cache file load.
- **Verify:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-desktop gh_status_cache` → pass (the literal compiles and the persisted round trip still loads).
  - `~/.claude/scripts/locked.sh desktop-ui bun run check` → 0 errors.
  - Add to a new `desktop-ui/src/lib/components/CheckEvidence.test.ts` (created in Step 5) a source assertion that `BranchCard.svelte` contains `pr-body-gaps`.
  - Manual: open a PR whose body lacks a "How to revert" heading; the card shows the note; edit the PR body on GitHub to add it; after the next status refresh the note is gone. Same in the TUI PR overview panel.
- **If it fails:** a compile error names a literal you missed.

### Step 3 — CI evidence parsers in the engine, test first
- **Goal:** pure functions for the job link, annotations and log tail.
- **Why / invariant:** `gh pr checks` gives no run or job id; the only handle is the `link` URL. Anything that is not a GitHub Actions job link must fall through to "external" with no `gh` call.
- **Files:** `crates/er-engine/src/pr_evidence.rs`.
- **Do this:**
  1. Tests first:
     - `parse_actions_job_link("https://github.com/o/r/actions/runs/123/job/456")` → `Some(ActionsJobRef { owner: "o", repo: "r", run_id: 123, job_id: 456 })`; same with a `?pr=7` query or `#step:3:10` fragment; `None` for `https://ci.example.com/build/1`, for a `/actions/runs/123` link with no job, and for a non-numeric id.
     - `parse_check_annotations` on a fixture array with two entries (`path`, `start_line`, `end_line`, `annotation_level`, `title`, `message`), one with `title: null` → two `CheckAnnotation`s, `title` empty for the null one; `"[]"` → empty; `"{}"` → error.
     - `log_tail` keeps the last N lines, reports `truncated = true` when it dropped any, enforces a byte cap without splitting a UTF-8 character, and strips ANSI colour sequences (`"\x1b[31mERR\x1b[0m"` → `"ERR"`).
  2. Implement:

     ```rust
     #[derive(Debug, Clone, PartialEq, Eq)]
     pub struct ActionsJobRef { pub owner: String, pub repo: String, pub run_id: u64, pub job_id: u64 }

     #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
     pub struct CheckAnnotation {
         pub path: String,
         pub start_line: Option<u64>,
         pub end_line: Option<u64>,
         pub level: String,
         pub title: String,
         pub message: String,
     }

     pub fn parse_actions_job_link(url: &str) -> Option<ActionsJobRef>;
     pub fn parse_check_annotations(json: &str) -> anyhow::Result<Vec<CheckAnnotation>>;
     pub fn log_tail(text: &str, max_lines: usize, max_bytes: usize) -> (String, bool);
     ```

     Limits as named constants: `LOG_TAIL_LINES = 200`, `LOG_TAIL_BYTES = 64 * 1024`, `MAX_ANNOTATIONS = 50`.
  - `TODO(executor): confirm the link shape. Run gh pr checks <n> --repo <owner/repo> --json name,link on a PR with GitHub Actions checks and check the link is https://github.com/<o>/<r>/actions/runs/<run>/job/<job>. If it differs, adjust the parser and its fixtures to the real shape.`
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pr_evidence` → all pass.
- **If it fails:** fix the code and leave the fixtures alone, unless this step's TODO showed the real link differs.

### Step 4 — The two `gh` calls and the per-job cache
- **Goal:** `pub fn fetch_check_evidence(link: &str) -> anyhow::Result<CheckEvidence>`.
- **Why / invariant:** runs only when a person clicks. A finished job's annotations and log never change (a re-run gets a new job id), so caching per job means a second click costs nothing.
- **Files:** `crates/er-engine/src/pr_evidence.rs`.
- **Do this:**

  ```rust
  #[derive(Debug, Clone, serde::Serialize)]
  #[serde(tag = "kind", rename_all = "snake_case")]
  pub enum CheckEvidence {
      /// Not a GitHub Actions job: details live on the CI provider.
      External,
      Actions {
          annotations: Vec<CheckAnnotation>,
          log_tail: String,
          log_truncated: bool,
          /// Set when the log could not be fetched; annotations may still be present.
          log_error: Option<String>,
      },
  }
  ```

  - `External` when `parse_actions_job_link` returns `None`; no `gh` call.
  - Otherwise run both in parallel with `crate::github::run_parallel`:
    - annotations: `gh api repos/{owner}/{repo}/check-runs/{job_id}/annotations?per_page=50`
    - log: `gh run view {run_id} --job {job_id} --log-failed --repo {owner}/{repo}` (flags confirmed against `gh run view --help`, gh 2.102.0).
    Both with `.output_timed(crate::proc::GH_TIMEOUT)`.
  - Annotations failure → empty list. Log failure → `log_error` with stderr. Both failing → `Err` with the log's stderr.
  - Cache successful results in a module-level map keyed by `(owner, repo, job_id)`, capped at 32 entries with oldest-first eviction, following `PrCommentsCache` (`github.rs:2000-2050`). Never cache an `Err`. Put the lookup in a small function that takes the fetch as a closure (`fn cached_evidence(key, fetch: impl FnOnce() -> Result<CheckEvidence>) -> Result<CheckEvidence>`) and unit-test it with a counting closure: the second call for the same key does not run the closure; an `Err` is not cached, so the next call runs it again; a 33rd key evicts the oldest. Use a test-only key prefix or a fresh cache instance so tests do not share state.
  - `TODO(executor): confirm that for GitHub Actions the job id equals the check-run id used by /check-runs/{id}/annotations — on a PR with a failing Actions check, compare gh api repos/<o>/<r>/actions/jobs/<job> --jq .check_run_url with the job id. If they differ, read the check-run id from that field (one extra call) and say so in Comments.`
- **Watch out for:** `gh run view --log-failed` can print a lot; always pass it through `log_tail`. It may fail on very old runs whose logs expired; that is the `log_error` path.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine cached_evidence` → the cache tests pass. Network behaviour is checked manually in Step 5.
- **If it fails:** a missing `run_parallel` means it moved; search `grep -n "fn run_parallel" crates/er-engine/src/github.rs`.

### Step 5 — Desktop command and the "Show failure" toggle
- **Goal:** a click on a failing check shows its evidence in place.
- **Why / invariant:** the fetch happens only on click, and the text from CI is untrusted, so it is shown as plain text.
- **Files:**
  - New `crates/er-desktop/src/check_evidence.rs`:

    ```rust
    #[tauri::command]
    pub async fn fetch_check_evidence(pr: PrTarget, check_url: String) -> Result<er_engine::pr_evidence::CheckEvidence, String> {
        run_blocking(move || fetch_check_evidence_blocking(&pr, &check_url)).await
    }
    ```

    `fetch_check_evidence_blocking` refuses (returns `Err`) when the link parses as an Actions job whose owner/repo differ from `pr` (case-insensitive), so the command can only read the PR's own repository; then calls `er_engine::pr_evidence::fetch_check_evidence(&check_url)`. Add a unit test for the refusal with a link to another repo. `run_blocking` is `pub` in `crates/er-desktop/src/commands.rs:246`; `PrTarget` is `pub` in `gh_pr_actions.rs:45`.
  - `crates/er-desktop/src/lib.rs` — `pub mod check_evidence;`.
  - `crates/er-desktop/permissions/app-commands.toml` — add `"fetch_check_evidence",` to `commands.allow` next to `"run_github_pr_action",` (line 59). The file's header says every application IPC command must be listed once explicit capabilities are on; a command missing here is refused when the frontend invokes it, and no unit test or `bun run check` catches that.
  - `crates/er-desktop/src/main.rs` — add `check_evidence` to the `use er_desktop::{…}` list (12-15) and `check_evidence::fetch_check_evidence,` to `generate_handler!` next to `gh_pr_actions::run_github_pr_action,` (1948).
  - `desktop-ui/src/lib/types.ts` — add `CheckAnnotation` and `CheckEvidence` types mirroring the Rust serde shape (`kind: "external" | "actions"`).
  - New `desktop-ui/src/lib/components/CheckEvidence.svelte`, props `{ github: GithubStatusSnapshot; check: CheckSummary }`:
    - a small "Show failure" text button; on first open call `app.cmd("fetch_check_evidence", { pr: prTarget(github), checkUrl: check.url })` (`prTarget` from `$lib/prMergeBox`, 330-332);
    - while loading show "Loading…"; on error show the error text;
    - `external` → "Details are on the CI provider." plus the existing link (`openExternalUrl`);
    - `actions` → annotations as `path:start_line — message` lines (title in front when non-empty), then the log tail in a `<pre>` with `max-h-64 overflow-auto font-mono text-[11px]`, with "(showing the last lines)" when `log_truncated`, and `log_error` when set;
    - keep the fetched result in component state so closing and reopening does not call again.
  - `desktop-ui/src/lib/components/BranchCard.svelte:518-541` — inside the `{#each github.checks as c, i (i)}` item, after the name button/span, render `<CheckEvidence {github} check={c} />` only when the check is failing and has a URL: `(c.conclusion === "FAILURE" || c.conclusion === "fail") && c.url` (the same spelling rule as `countChecks`, `desktop-ui/src/lib/checkCounts.ts:21-23`). The `li` is a flex row; wrap the name and the new component so the evidence block can take the full width below the name.
  - `desktop-ui/src/lib/components/CheckEvidence.test.ts` — source assertions in the `PrMergeBox.test.ts` style: the component calls `fetch_check_evidence` exactly once in its source; it renders the log with `{evidence.log_tail}` text interpolation and contains no `{@html`; `BranchCard.svelte` contains `<CheckEvidence` and `pr-body-gaps`.
- **Watch out for:** `app.cmd` argument names are camelCase on the frontend (`checkUrl`) and snake_case in Rust (`check_url`); the repo relies on this mapping (`desktop-ui/src/lib/components/ReplyActionBar.svelte:53` sends `threadId`). Keep BranchCard's added markup to the one component tag and the `if`; the logic belongs in `CheckEvidence.svelte`, since BranchCard is already 709 lines and `just lint-ui` enforces cohesion rules (ADR 0038).
- **Verify:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-desktop check_evidence` → passes.
  - `~/.claude/scripts/locked.sh desktop-ui bun run test` → passes, including `CheckEvidence.test.ts`.
  - `~/.claude/scripts/locked.sh desktop-ui bun run check` → 0 errors.
  - Manual: open a PR with a failing GitHub Actions check; expand checks; click "Show failure"; annotations and the failed log tail appear within a few seconds. Collapse and reopen: the result shows at once (component state); the engine cache test from Step 4 covers repeat fetches. On a PR whose failing check is an external CI, the toggle says the details are on the provider.
- **If it fails:** an "unknown command" or "not allowed" error from Tauri means the handler registration in `main.rs` or the `app-commands.toml` entry is missing.

## Gotchas & landmines
- **Never fetch logs from the status loop.** The 30-second loop already makes most of er's `gh` calls (`internal-docs/gh-rate-limit-slimming.md`, finding 1). Logs and annotations are fetched only from the click in Step 5.
- **Both spellings of failure.** `gh pr checks` reports `FAILURE` or the bucket `fail` depending on the source (`checkCounts.ts:12-15`). Use both, as `countChecks` does.
- **Untrusted text.** CI output and annotation messages come from the PR's code. Render as text; never `{@html}`.
- **Snapshot literals.** `GithubStatusSnapshot` has no `Default`; each literal needs the new field or the crate does not compile.
- **Moving `overview.body`.** In `fetch_github_status`, compute the gaps before `body: overview.body` moves the string.
- **`comments-check`** rejects comments citing plan steps or tracker numbers.

## Decision log (forks pre-answered)
- **Which sections:** Testing, Risk, How to revert, the three the blog names. Hard-coded with synonyms (Step 1.3). Making them configurable would mean new global config (ADR 0005) for a display-only note; skip it until someone asks.
- **Empty section counts as missing:** a heading with only a template comment means the author did not fill it in, which is what the reviewer needs to know.
- **Missing sections never block** approve or merge. The blog makes the person who asked for the change sign off; er's job is to show the gap.
- **TUI computes the gaps while rendering** by calling the engine function on `pr.body`. `crates/er-tui/src/ui/CLAUDE.md` prefers derived state on `TabState`, but `PrOverviewData` has 15 struct literals across three crates, and `render_pr_overview` already walks every body line each frame to word-wrap it, so the check adds no new order of cost. If a reviewer objects, add a cached field instead.
- **Logs: tail of failed steps only** (`--log-failed`), capped at 200 lines / 64 KB. Full logs stay one click away on GitHub.
- **Annotations and log both:** annotations give `file:line`, which is the most useful part; the log tail covers checks that emit no annotations.
- **Desktop only for Part B:** the TUI has no on-demand surface for a check; adding one is a separate change.
- **Cache without expiry, capped:** a finished job's log never changes, and a re-run gets a new job id.
- **What task 15 reads from this file (consistency pass, 2026-10-17):** 15's Evidence decision card reads `er_engine::pr_evidence::pr_body_gaps` (Step 1) and the existing check status (`CheckRun` / `CheckSummary`). It does not read `CheckEvidence`: logs and annotations stay click-only and desktop-only, as this file decides. Keep `pr_body_gaps` public and pure.

## Full verification
1. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pr_evidence` → all pass.
2. `~/.claude/scripts/locked.sh . cargo test -p er-desktop check_evidence` and `~/.claude/scripts/locked.sh . cargo test -p er-desktop gh_status_cache` → pass.
3. `~/.claude/scripts/locked.sh . just test` → all pass.
4. `~/.claude/scripts/locked.sh . just test-desktop` → all pass.
5. `~/.claude/scripts/locked.sh desktop-ui bun run test` and `~/.claude/scripts/locked.sh desktop-ui bun run check` → pass, 0 errors.
6. `~/.claude/scripts/locked.sh . just clippy` and `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
7. `~/.claude/scripts/locked.sh . just comments-check` → exit 0.
8. The manual checks in Steps 2 and 5.

In the final report, list every test added: the `pr_evidence` engine tests, the desktop refusal test, and `CheckEvidence.test.ts`.

## Rollback
Revert the commits. A status cache written by the new build still loads in the old one: `GithubStatusSnapshot` (`snapshot.rs:1373`) does not set `deny_unknown_fields`, so serde ignores the extra field.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- The Actions link shape or the job-id / check-run-id assumption turns out wrong in a way that needs more than one extra `gh` call per click.
- Anyone asks for the gap note to block approval or merge, or for configurable section names.
- The BranchCard change trips a `just lint-ui` cohesion rule you cannot satisfy by keeping the logic in `CheckEvidence.svelte`.

## Comments
