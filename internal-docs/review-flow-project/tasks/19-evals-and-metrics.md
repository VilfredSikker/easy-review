# 19 — Eval set and human-time metrics

Status: needs-design
Idea: 11 in the original idea list (summarised in ../README.md)
Depends on: 03-finding-outcomes.md. Optional, per metric: 08-review-convergence.md (rounds to pass). Task 09 keeps no record of fix runs, so "nits sent to a fixing agent" is always reported as not recorded (see the map).
Impact: H · Complexity: H · Size: XL
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — nobody can say whether a change to er's review prompts, model or provider made the reviewer better or worse. er records nothing about what happened to its findings over time, and has no fixed set of known bugs to test the reviewer against.
- **Why it matters** — it is easy to catch more bugs by flagging more non-bugs. Without a score, each prompt edit is a guess, and noise costs more when the author is an agent, because an agent fixes every nit and starts another round.
- **The fix** — three parts, built in order. (a) Local numbers from what people did with findings (task 03's outcomes): how often findings were fixed, wrong, intentional or didn't matter, per project; how many nits went to a fixing agent; how many rounds a PR needed to pass. (b) An eval set: past PRs that were reverted or caused an incident, with the bug's location confirmed by someone who knows the code, plus PRs known to be clean. er reruns a reviewer setup against the set and reports how many known bugs it caught and how many false alarms it raised on clean PRs. (c) Time a person spends per PR, which was cut once on purpose; it comes back only if the owner decides so.
- **TL;DR** — score er's reviewer on known bugs and on how people answered its findings, all on the local machine.

## Source
From *How the Claude Code team does code review*, https://claude.dev/blog/solving-code-review-at-anthropic/ ("How we make the reviewer better"):

> The team collected past PRs that caused incidents, with their fix-forwards and reverts, and had engineers who know that code confirm each bug and where it lived. The reviewer runs against that set daily.

> We sort every comment the reviewer leaves by how the person on the PR responded: they fixed it, said it was wrong, said the behavior was intentional, or decided it didn't matter. We also run the reviewer on PRs we know are clean and count how often it reports something that would have led someone to make a harmful change.

> Our priority, written down, is catching real bugs first and cutting review rounds second.

> Noise costs more when the author is an agent. A person can wave off a nit in seconds. An agent fixes it, which means another commit and another round of review.

## Mission & definition of done
Give er (a) a local metrics report built from recorded outcomes, (b) an eval runner that scores reviewer setups for recall on confirmed bugs and for false alarms on clean PRs, and (c) a written owner decision on human-time metrics. Everything stays on the user's machine. Step 1 is a design step that a person approves before any code.

- [ ] Step 1: ADR, `CONTEXT.md` terms and a compliance note written; owner approval and the owner's answer on human-time metrics recorded under `## Comments`.
- [ ] Phase A: an engine function computes per-project outcome rates from task 03's records, walking both the branch-bucket and the PR-bucket trees; an er-mcp tool returns it. Rounds-to-pass appears only when task 08 is done and is reported as unavailable otherwise. Nits-to-fixing-agent is reported as unavailable, because task 09 keeps no record of fix runs.
- [ ] Phase B: eval cases load from a directory; `er eval` runs one or more reviewer setups over every case in a temporary detached worktree at the case's head commit, takes an agent slot per run, and writes a local results file with recall and clean-PR false-alarm counts.
- [ ] Nothing in this task sends data off the machine, adds telemetry, or exports provider outputs in a form meant for training.
- [ ] Phase C is built only if the owner chose it in Step 1.

**Risk:** high. It touches the compliance line in `docs/ai-provider-compliance.md` (training on provider outputs) and adds a command that spends provider credits in bulk.

## Before you start (orientation)
- **Branch:** see `../README.md` "Branching". Feature work: current release branch.
- **Prerequisite:** task 03 is `Status: done`. Check `tasks/03-finding-outcomes.md`. If not, stop.
- **Commands:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-engine <filter>`
  - `~/.claude/scripts/locked.sh . cargo test -p er-mcp <filter>`
  - `~/.claude/scripts/locked.sh . cargo test -p er-tui <filter>`
  - `~/.claude/scripts/locked.sh . just build-engine-headless`, `~/.claude/scripts/locked.sh . just clippy`, `just docs-check`, `just comments-check`
- **The map** (confirmed at c96dc0f12 unless marked):
  - Outcome records (03-finding-outcomes.md Step 2): `crates/er-engine/src/ai/outcomes.rs`, re-exported from `er_engine::ai`; one `<er_dir>/finding-outcomes.json` per view bucket. Read it with `load_finding_outcomes(er_dir: &str) -> FindingOutcomes` (absent or malformed reads as empty). `FindingOutcomes { version, outcomes: Vec<FindingOutcome> }`; each `FindingOutcome` copies `file`, `line_start`, `title`, `category`, `lens`, `raised_by`, `severity`, `confidence`, `outside_diff`, plus `outcome: OutcomeKind`, `reason`, `diff_hash`, `recorded_at`. `OutcomeKind { Fixed, Wrong, Intentional, DoesNotMatter }`, serialized `"fixed" | "wrong" | "intentional" | "does_not_matter"`; use those names in `ProjectMetrics`. `ai/` is always compiled, so er-mcp can read it. Findings can be removed from sidecars (`crates/er-engine/src/ai/finding_cleanup.rs:80` `pub fn remove_finding_from_sidecars(er_dir: &str, finding_id: &str) -> std::io::Result<bool>`), so metrics must read task 03's record, never count surviving findings.
  - Round records (08-review-convergence.md Step 2): a host-written `<er_dir>/rounds.json` per view bucket, read with `crate::ai::load_rounds(er_dir) -> ReviewRounds` from `crates/er-engine/src/ai/rounds.rs`. `ReviewRounds` holds `rounds: Vec<Round>` (each with `n`, `kind: RoundKind`, `passed`, `blocking_open`, `not_posted`) and `rounds_to_pass`, the `n` of the first passing round (`null` in the file until a round passes). Count only rounds whose kind is `Full`, `ChangesOnly` or `FullByHand`; task 14 adds a `Live` kind that does not count. Check it landed with `grep -n "pub fn load_rounds" crates/er-engine/src/ai/rounds.rs`.
  - Fix-action records: none. 09-answer-every-finding.md's Decision log says its fix action sends open must-fix findings only, never nits, and `fix-findings.json` is overwritten on every run, so no durable record exists. Report `nits_to_fixing_agent` as `None` ("not recorded"). Do not add a record to task 09's code here.
  - `crates/er-engine/src/sidecar_summary.rs:195` `pub fn list_repo_pr_artifacts(owner: &str, repo: &str, kinds_filter: Option<&[SidecarKind]>, limit: usize) -> Vec<ListedPrArtifacts>` and `:207` `pub fn list_repo_pr_artifacts_in_dir(owner, repo, prs_dir: &Path, kinds_filter, limit)` — the walker over `prs/pr-*` buckets, with a test-friendly `_in_dir` variant. Copy this split.
  - `crates/er-engine/src/storage.rs:14` `storage_root()` (honours `ER_STORAGE_ROOT`), `:51` `pub fn rules_key(repo_root: &str, remote_repo: Option<&str>) -> String` (the branch-tree repo slug from `owner/name` without a clone), `:136` `view_bucket_dir(repo_slug, branch_slug, bucket)` → `repos/<repo>/branches/<branch>/view-buckets/<bucket>`. PR tree: `crates/er-engine/src/github.rs:3243` `pub fn owner_repo_storage_slug(owner: &str, repo: &str) -> String` → `repos/<owner-repo>/prs/pr-<N>`.
  - `crates/er-mcp/src/server.rs:738` `async fn pr_stats(&self, Parameters(args): Parameters<PrStatsArgs>)` — diff size stats only (production vs test lines). `:92` `pub struct PrStatsArgs`. Helpers `text_json` (`:234`), `tool_err` (`:240`), `query_repo_args` (`:319`), `projects::resolve_repo` (`crates/er-mcp/src/projects.rs:96`). Tests: `mod tests` at `:1429`, e.g. `registers_pr_diagram_tool`.
  - `crates/er-engine/src/arena/adapter.rs:29` `pub fn resolve_provider_command(hub: &AiHubConfig, provider_id: &str, model_id: &str, effort: Option<&str>, storage_dir: Option<&str>) -> Result<ProviderCommand>`; `:64` `pub fn run_provider_json(cmd: &ProviderCommand, prompt: &str, work_dir: &str, cancel: &AtomicBool, children: &Arc<Mutex<Vec<Child>>>) -> Result<Value>` — synchronous, headless, returns the provider's stdout JSON. It honours `ER_FAKE_ARENA_DIR` (`:71`) for tests. It does **not** take an agent slot; the orchestrator does that (`crates/er-engine/src/arena/orchestrator.rs:816`, `crate::agent_slots::acquire(Workload::Arena, arena_cap, ceiling, &cancel)`).
  - `crates/er-engine/src/agent_slots.rs:202` `pub fn acquire(workload: Workload, cap: usize, ceiling: usize, cancel: &AtomicBool) -> Option<AgentSlotGuard<'static>>`; `enum Workload { Background, Arena }` at `:31`.
  - `crates/er-engine/src/ai/prompts.rs:1823` `pub fn build_arena_round1_prompt_agent(diff_patch_path: &str, reviewer_label: &str, agent_kind: Option<&str>) -> String` — a finder prompt that prints `{"findings":[{"file","line","title","body","severity","confidence","tags"}]}` on stdout. Task 02 changes its rules; that is expected. Task 05 Step 6 adds a fourth parameter, `review_md: Option<&str>`; if 05 has landed, pass `None` (an eval case has no base-branch `REVIEW.md` to read).
  - `crates/er-engine/src/ai/prepared_diff.rs:38` `pub fn ensure_diff_artifacts(er_dir: &str, raw: &str) -> Result<String, String>` — writes `diff-tmp` etc. and returns the diff hash.
  - `crates/er-engine/src/config/mod.rs:432` `pub struct AiSelection { provider_id: Option<String>, model_id: Option<String>, effort: Option<String> }`.
  - `crates/er-engine/src/agent_timing.rs` — off-by-default wall-clock instrumentation (`ER_AGENT_TIMING=1`, stderr only). Not a metrics store; do not build on it.
  - Fake provider fixtures: `tests/fixtures/arena/fake/round1.json` (findings on `src/lib.rs` lines 10 and 12), used by `crates/er-engine/tests/arena_timing.rs:60` and `crates/er-engine/src/arena/adapter.rs:584` (`fake_arena_dir_round_robin`, which serialises on `ARENA_TEST_ENV_LOCK`).
  - CLI: `crates/er-tui/src/main.rs:67` `enum Commands { Uninstall {…}, Update }`; `Update` returns before terminal setup (`:187`).
  - Feature gates: `crates/er-engine/src/lib.rs:9–12` — `app` and `arena` are `#[cfg(feature = "ui")]`. er-mcp builds er-engine with `default-features = false` (`crates/er-mcp/Cargo.toml:15`), so er-mcp can use metrics code only if it lives outside `app` and `arena`.
  - `docs/ai-provider-compliance.md` — "Lines we will not cross" and "Triggers that change this analysis".
  - Throughput stats were cut in `.scratch/review-triage-pipeline/spec.md` (local and gitignored; in the main checkout at `/Users/vilfredsikker/Projects/easy-review/.scratch/review-triage-pipeline/spec.md`, "Cut, deliberately"): "**Throughput stats.** Median time-to-merge by lane, share merged under 1h. Deprioritised; `pr_stats` is the starting point when it returns."
- **Conventions to follow** — the walker split, from `crates/er-engine/src/sidecar_summary.rs:195`:

  ```rust
  let slug = owner_repo_storage_slug(owner, repo);
  let prs_dir = storage_root().join("repos").join(&slug).join("prs");
  list_repo_pr_artifacts_in_dir(owner, repo, &prs_dir, kinds_filter, limit)
  ```

  and the fake provider hook, from `crates/er-engine/src/arena/adapter.rs:71`:

  ```rust
  if let Ok(dir) = std::env::var("ER_FAKE_ARENA_DIR") {
      return fake_arena_json_from_dir(&dir);
  }
  ```
- **Mental model** — Every review artifact lives in managed storage, split into branch view buckets (`repos/<repo>/branches/<branch>/view-buckets/<bucket>`) and PR buckets (`repos/<owner-repo>/prs/pr-<N>`), with two different repo slugs. Task 03 adds an outcome record when a person answers a finding; Phase A only reads those records. Phase B is separate from the app: a headless command that replays a past PR through one finder prompt on a chosen provider and model, and compares the findings with the confirmed bug location. The prompts are compiled in (ADR 0023), so "which prompt" is the er build that ran it.
- **ADRs to read first:**
  - 0003 — managed storage; metrics and eval results live there too.
  - 0004 — the two bucket trees a metrics walk must cover.
  - 0005 — config is global-only; eval settings are command-line flags, not a repo file.
  - 0009 — findings are AI-owned and can be removed, which is why outcomes are a separate record.
  - 0021 — any agent spawn path must take a slot; the eval runner calls `run_provider_json` directly, so it must acquire one itself.
  - 0023 — prompts compiled in; the er version identifies the prompt.
  - 0024 — arena quorum; context for the round-1 finder the eval runner reuses.

## Scope boundaries
- **In scope:** ADR, `CONTEXT.md`, a compliance note; Phase A metrics module and er-mcp tool; Phase B case format, loader, scorer, runner and `er eval` subcommand; Phase C only if chosen.
- **OUT of scope / DO NOT TOUCH:**
  - Any upload, telemetry, analytics SDK, or network call other than the provider CLIs the user already runs.
  - Exporting findings, outcomes or eval outputs as a dataset for fine-tuning, or any training pipeline.
  - Publishing or rendering a provider-versus-provider ranking for use outside the machine.
  - Automatic changes to review depth from metrics (the "circuit breaker" in the original idea list). Report-only.
  - `pr_stats`: do not add outcome metrics to it; it is a diff-size tool.
  - Building cases automatically from git history. Cases are written by people who confirmed the bug.
  - The desktop and TUI UI for metrics. A UI is a follow-up once the numbers prove useful.

## Steps
### Step 1 — ADR, terms, compliance note, owner decisions; then stop
- **Goal:** owner approves the metric definitions, eval case format and storage, the compliance invariant, and answers the human-time question.
- **Why / invariant:** the compliance line must be written down before code makes it easy to cross: scoring a reviewer with provider outputs is allowed; training a model on them is not (`docs/ai-provider-compliance.md`, "Lines we will not cross", 1).
- **Files:** new `docs/adr/<next>-review-evals-and-outcome-metrics-stay-local.md`; `CONTEXT.md`; `docs/ai-provider-compliance.md`.
- **Do this:**
  1. `CONTEXT.md`, after **AI Hub**:
     - **Eval case**: a past change with a known answer: either a bug confirmed by someone who knows that code, with the file and lines where it lived, or a change known to be clean. _Avoid_: test case, fixture, benchmark.
     - **Eval run**: one reviewer setup (er build, provider, model, effort, finder prompt) run over every eval case, with its scores. _Avoid_: benchmark, test run.
     - **Recall**: in an eval run, the share of bug cases where at least one finding of `medium` or `high` severity lands on a confirmed location. _Avoid_: accuracy, hit rate.
     - **False alarm**: in an eval run, a `high` severity finding on a clean case. _Avoid_: false positive (it is used loosely elsewhere), noise.
  2. ADR, each decision with its reason:
     - Priority: recall first, review rounds second (blog quote).
     - Metrics are computed on demand from task 03/08/09 records; no new metrics store.
     - Cases live in a directory passed to `er eval --cases <dir>`, defaulting to `<storage_root>/evals/<repo-slug>/cases/`. A team that wants shared cases keeps them in any directory it likes; er does not read a repo-local file by default (ADR 0005 spirit).
     - Each case runs in a temporary detached `git worktree` at its head commit, removed afterwards, so the agent reads the code as it was.
     - The eval runner takes an agent slot per provider call (ADR 0021).
     - Scoring rule: a hit is a finding on a confirmed file within ±3 lines of a confirmed range (configurable by flag).
     - Results: `<storage_root>/evals/<repo-slug>/runs/<unix-ms>.json`, local only.
  3. `docs/ai-provider-compliance.md`: under "Lines we will not cross", add one item: "4. **Evals and outcome metrics score er; they never become training data.** Eval runs and outcome metrics stay on the user's machine. No feature exports provider outputs, outcomes or eval results as a corpus for training or fine-tuning a model, and cross-provider eval scores are for private comparison only (see line 3)." Update `_Last reviewed:_` to the date you edit it.
  4. Append to `## Comments` here: the ADR path, terms, and the **owner decision request for Phase C** with these options:
     - **C0 — keep cut.** No human-time metrics (the current state).
     - **C1 — time-to-merge from GitHub.** Per PR, `createdAt` → `mergedAt` via `gh`, extending `pr_stats`'s batch mode (this is what the quoted review-triage-pipeline spec named as the starting point). No local tracking.
     - **C2 — local time-in-review.** er records, locally, when a person opened and closed a PR tab and how long it had focus. This is new local data about the user; it stays on disk, but it still needs an explicit opt-in setting and must never be uploaded (compliance doc, telemetry trigger).
     Recommend C0 until Phase A has a month of data, then C1.
  5. `just docs-check`. Stop.
- **Verify:** `just docs-check` → exit 0. `grep -n "Evals and outcome metrics" docs/ai-provider-compliance.md` → one match.
- **If it fails:** fix the docs only.

**STOP here until the owner approves and answers Phase C.**

### Phase A — metrics from recorded outcomes

### Step 2 — Metrics module with a test-friendly walker
- **Goal:** `crates/er-engine/src/review_metrics.rs` (always compiled; register as `pub mod review_metrics;` in `crates/er-engine/src/lib.rs` beside `pub mod sidecar_summary;`).
- **Why / invariant:** er-mcp must be able to call it, so it cannot import `crate::app` or `crate::arena`. It must cover both bucket trees.
- **Do this:**
  - `pub fn project_metrics(owner: &str, repo: &str) -> ProjectMetrics` and `pub fn project_metrics_in_dirs(branches_dir: &Path, prs_dir: &Path) -> ProjectMetrics`. The first computes `branches_dir = storage_root()/repos/<rules_key("", Some("owner/repo"))>/branches` and `prs_dir = storage_root()/repos/<owner_repo_storage_slug(owner, repo)>/prs`, then calls the second.
  - Walk `branches/*/view-buckets/*` and `prs/pr-*`; in each bucket read task 03's outcome records. Task 17 (Step 2) walks the same two trees for the same records and, in the recommended order, lands first. If it has, call its record walk instead of writing a second one; if not, write the walk as a separate always-compiled function that returns the records, so 17 can reuse it.
  - `ProjectMetrics`: total answered findings; counts per outcome (fixed, wrong, intentional, didn't matter — use task 03's names); the same split by severity and by lens; `rounds_to_pass: Option<RoundsSummary>` (mean, share passing after one round, share needing five or more) when task 08's records exist, else `None`; `nits_to_fixing_agent: Option<u64>`, always `None` for now, because task 09 keeps no record (see the map).
  - Unknown or unreadable files are skipped and counted in a `skipped: u64` field; never fail the whole report.
- **Watch out for:** the two slugs differ (`rules_key` gives the origin basename; `owner_repo_storage_slug` gives `owner-repo`). A walk of one tree only undercounts silently.
- **Verify:** tests in `review_metrics.rs`, building a temp tree with `tempfile::tempdir()` (the `sidecar_summary.rs` tests at `:312` show the pattern):
  - `metrics_count_outcomes_from_both_bucket_trees` (one record in a branch bucket, one in a PR bucket → total 2);
  - `missing_round_records_report_none` ;
  - `an_unreadable_record_is_skipped_and_counted`.
  `~/.claude/scripts/locked.sh . cargo test -p er-engine review_metrics::` → 3 passed. Plant a break (skip the branches walk), rerun, see the first test fail, revert in the same command. `~/.claude/scripts/locked.sh . just build-engine-headless` → builds.
- **If it fails:** stop if task 03's record cannot be read without `app`.

### Step 3 — er-mcp tool `review_metrics`
- **Goal:** a new tool returning `ProjectMetrics` as JSON for a repo or project.
- **Why / invariant:** `pr_stats` answers "how big is this diff"; widening it to carry outcome metrics would give one tool two meanings.
- **Files:** `crates/er-mcp/src/server.rs` — an args struct like `PrStatsArgs` (`:92`) with `#[serde(flatten)] target: PrRefFields`; a `#[tool(description = "…")] async fn review_metrics(...)` beside `pr_stats` (`:738`), resolving the repo with `query_repo_args` + `projects::resolve_repo` exactly as `pr_stats`'s batch branch does, then `tokio::task::spawn_blocking` around `project_metrics`, then `text_json`.
- **Verify:** add `registers_review_metrics_tool` in `mod tests` (`:1429`), copying `registers_pr_diagram_tool`. `~/.claude/scripts/locked.sh . cargo test -p er-mcp review_metrics` → 1 passed. Then `~/.claude/scripts/locked.sh . cargo test -p er-mcp` → all pass.
- **If it fails:** stop.

### Phase B — eval set and runner

### Step 4 — Eval case format and loader
- **Goal:** `crates/er-engine/src/eval/case.rs` in a new module `eval` (`crates/er-engine/src/eval/mod.rs` with `pub mod case; pub mod score; #[cfg(feature = "ui")] pub mod run;`, registered as an always-compiled `pub mod eval;` in `crates/er-engine/src/lib.rs`). `case` and `score` only parse and compare JSON, so they stay outside the `ui` gate.
- **Do this:**
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct EvalCase {
      pub id: String,
      pub repo: String,                 // owner/name
      pub base: String,                 // commit sha
      pub head: String,                 // commit sha
      pub kind: EvalCaseKind,           // "bug" | "clean"
      #[serde(default)] pub source: String,       // "revert" | "incident" | "clean" | free text
      #[serde(default)] pub pr: Option<u64>,
      #[serde(default)] pub locations: Vec<EvalLocation>, // required non-empty for "bug"
      #[serde(default)] pub confirmed_by: String,          // required non-empty for "bug"
      #[serde(default)] pub note: String,
  }
  pub struct EvalLocation { pub path: String, pub line_start: usize, pub line_end: usize }
  ```
  `pub fn load_cases(dir: &Path) -> (Vec<EvalCase>, Vec<String>)` returns valid cases and one error string per rejected file (bug case without locations or `confirmed_by`, `line_end < line_start`, unsafe path).
- **Verify:** tests `a_bug_case_without_a_confirmed_location_is_rejected`, `a_clean_case_needs_no_location`, `cases_load_from_a_directory`. `~/.claude/scripts/locked.sh . cargo test -p er-engine eval::case` → 3 passed.
- **If it fails:** fix the module.

### Step 5 — Scorer (pure)
- **Goal:** `crates/er-engine/src/eval/score.rs`: `pub fn score_case(case: &EvalCase, findings: &[EvalFinding], tolerance: usize) -> CaseScore` and `pub fn summarize(scores: &[CaseScore]) -> RunSummary` (recall over bug cases, false alarms over clean cases, counts of all findings by severity).
- **Why / invariant:** the score must be reproducible from stored findings, so a scoring change can be rerun without spending credits.
- **Do this:** `EvalFinding { path, line, severity }` mapped from the round-1 JSON (`file`, `line`, `severity` of `high|med|low`). Hit: severity `high` or `med`, same path, `line` within `[line_start - tolerance, line_end + tolerance]`. False alarm: severity `high` on a clean case.
- **Verify:** tests `a_finding_inside_the_range_is_a_hit`, `a_low_severity_finding_is_not_a_hit`, `a_high_finding_on_a_clean_case_is_a_false_alarm`, `recall_is_over_bug_cases_only`. `~/.claude/scripts/locked.sh . cargo test -p er-engine eval::score` → 4 passed. Plant a break (drop the tolerance), see `a_finding_inside_the_range_is_a_hit` fail with a test that sits one line outside the range, revert in the same command.
- **If it fails:** fix the scorer.

### Step 6 — Runner (behind the `ui` feature)
- **Goal:** `crates/er-engine/src/eval/run.rs`, gated `#[cfg(feature = "ui")]` because it uses `crate::arena::adapter`.
- **Why / invariant:** each provider call takes an agent slot (ADR 0021); the agent reads the code at the case's head; nothing leaves the machine except the provider call the user's own CLI makes.
- **Do this:** `pub fn run_eval(repo_root: &str, cases: &[EvalCase], setups: &[AiSelection], hub: &AiHubConfig, tolerance: usize, out_dir: &Path, cancel: &AtomicBool) -> Result<PathBuf>`. For each setup × case:
  1. `git worktree add --detach <tmp> <head>` in `repo_root` (fail the case, not the run, if the commit is missing; tell the user to fetch it).
  2. `git diff <base> <head> --unified=20 --no-color --no-ext-diff` → `ensure_diff_artifacts(tmp_er_dir, &raw)` with `tmp_er_dir` a fresh temp dir outside the worktree.
  3. Prompt: `build_arena_round1_prompt_agent(&format!("{tmp_er_dir}/diff-tmp"), "eval", Some("general"))`, plus a trailing `None` if task 05 has added its `review_md` parameter.
  4. `let _slot = crate::agent_slots::acquire(Workload::Arena, cap, ceiling, cancel)` with `cap = hub.effective_max_concurrent_arena_reviews()` and `ceiling = hub.effective_max_concurrent_agents()`, as `orchestrator.rs:813–814` does. `None` means cancelled: stop the run.
  5. `resolve_provider_command(hub, provider, model, effort, Some(tmp_er_dir))`, then `run_provider_json(&cmd, &prompt, <worktree path>, cancel, &children)`.
  6. Map findings, `score_case`, keep the raw findings for re-scoring.
  7. `git worktree remove --force <tmp>` in every exit path (use a guard struct with `Drop`).
  Write `out_dir/<unix-ms>.json` with: er version (`env!("CARGO_PKG_VERSION")`), each setup, tolerance, per-case findings and scores, and the summary.
- **Watch out for:** `run_provider_json` retries on rate limits and reads `ER_FAKE_ARENA_DIR`; the variable is process-global. `ARENA_TEST_ENV_LOCK` (`adapter.rs:331`) is private to that file's `mod tests`, so it does not reach an integration test. The integration test in `crates/er-engine/tests/eval_run.rs` runs in its own process; keep it the only test in that file that sets `ER_FAKE_ARENA_DIR`, or add a file-local `static` mutex the same way.
- **Verify:** an integration test in `crates/er-engine/tests/eval_run.rs`, modelled on `crates/er-engine/tests/arena_timing.rs`: build a temp git repo with `src/lib.rs`, two commits; a bug case located at `src/lib.rs` lines 10–10; set `ER_FAKE_ARENA_DIR` to `tests/fixtures/arena/fake` (its `round1.json` has a `high` finding on `src/lib.rs` line 10); run; assert recall is 1.0 and that no worktree is left (`git worktree list` has one entry). `~/.claude/scripts/locked.sh . cargo test -p er-engine --test eval_run` → 1 passed.
- **If it fails:** stop after three attempts and report.

### Step 7 — `er eval` subcommand
- **Goal:** `er eval [--cases <dir>] --setup <provider>:<model>[:<effort>] [--setup …] [--tolerance 3] [--rescore <results.json>]`.
- **Files:** `crates/er-tui/src/main.rs:67` (`enum Commands`: add `Eval { … }`), dispatch beside `Update` (`:186`). Load global config with `er_engine::config::load_global_config()` (`crates/er-engine/src/config/mod.rs:1831`, returns `ErConfig`), and use its `ai_hub`. The subcommand needs no `App` and must return before `App::new_with_args` (`main.rs:272`).
- **Do this:** before running, print the case count × setup count and an estimated cost, and wait for `y` unless `--yes` (Deep-asks-first spirit: bulk provider spend needs a click). `--rescore` re-scores a stored results file with a new tolerance and spends nothing. Print the summary table; exit 0 even when recall is low (it is a report).
- **Verify:** `~/.claude/scripts/locked.sh . cargo test -p er-tui` → passes. Manual: `ER_FAKE_ARENA_DIR=$PWD/tests/fixtures/arena/fake cargo run -p er-tui -- eval --cases <dir with the Step 6 case> --setup claude:sonnet --yes` → prints recall 1/1 and a results path under `<storage_root>/evals/`.
- **If it fails:** stop.

### Phase C — human-time metrics

### Step 8 — Only if the owner chose C1 or C2 in Step 1
- **Goal:** build exactly the option chosen. For C1, extend `pr_stats`'s batch mode with `created_at`, `merged_at` and the hours between, from `gh pr view --json createdAt,mergedAt` (ADR 0026: `gh` CLI, no HTTP client). For C2, stop and ask for a design: it creates new local data about the user and needs its own ADR and an opt-in setting.
- **Verify:** for C1, a test in `crates/er-mcp` for the new fields' parsing. `~/.claude/scripts/locked.sh . cargo test -p er-mcp` → passes.
- **If it fails:** stop.

## Gotchas & landmines
- **Two repo slugs** (`rules_key`/`slug_repo` vs `owner_repo_storage_slug`). Step 2's first test exists to catch a one-tree walk.
- **Outcomes outlive findings.** A finding can be removed from its sidecar; only task 03's record keeps what happened to it.
- **`run_provider_json` takes no slot.** Acquire one around each call, or an eval run multiplies provider processes past the cap the user set.
- **`ai`, `eval::case`, `eval::score` and `review_metrics` are always compiled; `eval::run` is not.** Keep the `#[cfg(feature = "ui")]` on `run.rs` only, or er-mcp stops building. Check with `just build-engine-headless`.
- **The worktree must be removed on every path**, including a panic in scoring and a cancelled slot wait. Use a `Drop` guard.
- **A case's commits may not be in the local clone.** Fail that case with a fetch hint; do not fetch automatically.
- **Prompt identity is the er build.** Two runs on different er versions compare prompts as well as models; the results file records the version so a reader can tell.

## Decision log (forks pre-answered)
- **If you want to store metrics in a new file that updates on each outcome:** don't. Compute on demand from task 03's records; a second copy drifts.
- **If you want the eval runner to drive the full default flow from task 07:** not in this task. The round-1 finder is headless today; add the full flow as another setup kind once task 07 exposes a headless entry, and record that under `## Comments`.
- **If you want to keep the raw provider outputs in results:** keep only the mapped findings (path, line, severity, title) needed to re-score; not the full transcript. This keeps the results file small and away from a training-corpus shape.
- **If a team wants shared cases:** they pass `--cases` pointing at a directory they keep wherever they like. er does not read cases from the reviewed repo by default.
- **If a metric's source task (08) is not done, or keeps no record (09):** return `None` for it and say "not recorded" in the tool output. Never estimate it.
- **Consistency pass (2026-10-17):** the record names come from 03 Step 2 and 08 Step 2. Task 09 sends no nits and keeps no record of fix runs (09 Decision log), so `nits_to_fixing_agent` stays `None` and 09 is not a dependency. Task 05 adds a fourth argument to `build_arena_round1_prompt_agent`. Task 17 shares the outcome walk. Whether to add a durable fix-run record so this metric can exist is a decision for the owner.
- **Human-time metrics:** never build without the owner's Step 1 answer.

## Full verification
1. `~/.claude/scripts/locked.sh . cargo test -p er-engine review_metrics::` → 3 passed.
2. `~/.claude/scripts/locked.sh . cargo test -p er-engine eval::` → 7 passed.
3. `~/.claude/scripts/locked.sh . cargo test -p er-engine --test eval_run` → 1 passed.
4. `~/.claude/scripts/locked.sh . cargo test -p er-mcp` → all pass, including `registers_review_metrics_tool`.
5. `~/.claude/scripts/locked.sh . cargo test -p er-engine` and `~/.claude/scripts/locked.sh . cargo test -p er-tui` → pass.
6. `~/.claude/scripts/locked.sh . just build-engine-headless` → builds.
7. `~/.claude/scripts/locked.sh . just clippy` → exit 0.
8. `just docs-check` → exit 0; `just comments-check` → exit 0.

## Rollback
Revert the commits. Eval cases and results live under `<storage_root>/evals/` (or the user's `--cases` directory) and are never read by any other feature. The compliance-doc line can stay even if the code is reverted; remove it only if the owner asks. Leftover temporary worktrees, if a run was killed hard: `git worktree prune` in the repo.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- Task 03 is not done, or its records cannot be read from outside `app`.
- After Step 1 until the owner approves and answers Phase C.
- Anything would send data off the machine, add telemetry, or produce an export of provider outputs.
- The owner asks for a published comparison of providers: compliance doc line 3 requires checking each provider's benchmarking terms first.
- The owner asks for automatic depth changes from metrics.

## Comments
