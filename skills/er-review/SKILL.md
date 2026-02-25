# er-review

Quick AI code review for the current git diff, producing `.er-*` sidecar files that the `er` TUI reads.

## Trigger

Run as `/er-review` or `/er-review [scope] [base-branch]`.

## Arguments

`/er-review [scope]`

- `branch` or `1` (default) — full branch diff: `git diff <base> --unified=3 --no-color --no-ext-diff`
- `unstaged` or `2` — uncommitted changes: `git diff --unified=3 --no-color --no-ext-diff`
- `staged` or `3` — staged changes: `git diff --staged --unified=3 --no-color --no-ext-diff`

Numbers match the er TUI keybindings (1/2/3 to switch modes).

A base branch can optionally follow the scope: `/er-review branch develop`.
If no base branch is given, detect main or master.

## Diff source — CRITICAL

Both `/er-review` and `/review-pr` MUST use the same diff command that `er` uses internally:

```
git diff <base> --unified=3 --no-color --no-ext-diff
```

**This is a two-dot diff** (`git diff main`), comparing the base branch to the **working tree**.
Do NOT use three-dot (`git diff main...HEAD`) — that compares to HEAD only and produces a
different hash, causing `er` to show the review as stale.

For `unstaged` and `staged` scopes, the base branch is irrelevant — the diff is against the index.

## What it does

1. Reads the diff for the selected scope
2. Computes a SHA-256 `diff_hash` of the raw diff for staleness detection
3. Checks persistence cache for reusable prior review
4. Analyses every changed file and hunk (single-pass, no agents) — incrementally if a base review exists
5. Writes four files to the repo root:
   - `.er-review.json` — per-file risk levels, summaries, and findings pinned to hunks
   - `.er-order.json` — suggested review order grouped by logical concern
   - `.er-checklist.json` — actionable review checklist items
   - `.er-summary.md` — human-readable overall summary
6. Persists the review to `.er-reviews/<branch>/<commit-hash>/` for incremental reuse

## Feedback-aware mode

Before generating, check if `.er-feedback.json` exists and its `diff_hash` matches the current diff. If it does, read the human comments and:
- Address each comment in the relevant finding's `responses` array
- Add new findings if a comment reveals something you missed
- Archive the old feedback to `.er-feedback.prev.json`

If `.er-feedback.json` exists but its `diff_hash` doesn't match, ignore it (it's stale).

## Speed budget

**Target: ≤15 tool calls total, ≤90 seconds.** Every tool call costs ~3s round-trip.
The diff is read into context ONCE. All analysis happens in-context (zero tool calls).
Outputs are written in parallel. Bash calls are batched with `&&`.

### Permission & hook constraints

All Bash commands MUST start with an allowed command to avoid permission prompts.
Allowed first-words: `git`, `shasum`, `mkdir`, `cp`, `scripts/er-*`
NOT allowed as first word: `for`, `rm`, `while`, `bash`, `sh`

- `&&` chaining is FINE for: `git`, `shasum`, `printf`, `mkdir`, `cp` (not in CHAIN_BLOCKED)
- `rm` IS in CHAIN_BLOCKED — CANNOT appear after `&&`. Leave .er-diff-tmp in place (gitignored).
- Do NOT pipe (`|`) into `shasum` — use file-based hashing: `shasum -a 256 <file>`
- Use `scripts/er-hash-files.sh <scope-args>` for per-file hashing (avoids `for` loop)
- Use `scripts/er-freshness-check.sh <base>` for base validation + diff capture + hash

## Step-by-step

```
Step 1: Scope + fast-path check (2-3 tool calls)

Parse scope from arguments (default: branch):
- "branch" or "1" → branch scope
- "unstaged" or "2" → unstaged scope
- "staged" or "3" → staged scope

Base branch: use second argument if provided, else detect main/master.

Scope args (used in all git diff commands below):
- branch:   main --unified=3 --no-color --no-ext-diff
- unstaged: --unified=3 --no-color --no-ext-diff
- staged:   --staged --unified=3 --no-color --no-ext-diff

TOOL CALL 1 — Bash (all setup via helper script):
  For branch scope:
    scripts/er-freshness-check.sh <base>
    → Output: "ok", hash line, commit hash (3 lines)
  For unstaged/staged scope (no base branch):
    git diff <scope-args> > .er-diff-tmp && shasum -a 256 .er-diff-tmp && git rev-parse --short HEAD && git branch --show-current
  → Captures: diff_hash, commit_hash, branch_name
  → Both forms match allow rules: scripts/er-* or git diff *

TOOL CALL 2 — Read .er-review.json (if it exists):
  → If exists AND diff_hash matches → print "Review is current", DONE (2 calls total)
  → .er-diff-tmp is left in place (overwritten next run, gitignored)

Step 2: Check persistence cache (1-2 tool calls)

TOOL CALL 3 — Read .er-reviews/<branch>/<commit-hash>/.er-review.json:
  → If exists AND diff_hash matches:
    TOOL CALL 4 — Bash: cp .er-reviews/<branch>/<commit>/.er-review.json .er-reviews/<branch>/<commit>/.er-order.json .er-reviews/<branch>/<commit>/.er-checklist.json .er-reviews/<branch>/<commit>/.er-summary.md .
    → Print "Restored cached review", DONE (4 calls total)
  → If exists but diff_hash mismatched: use as incremental base (Step 4)
  → If not found, check most recent in .er-reviews/<branch>/ for incremental base
  → If nothing found → full analysis (Step 3)

Step 3: Full analysis (~10 tool calls total)

TOOL CALL 4 — Bash (per-file hashes via helper script):
  scripts/er-hash-files.sh <scope-args>
  → Outputs: <file>\t<hash>  .er-diff-tmp per line. Parse file name and hash.
  → Matches Bash(~/.claude/scripts/*) allow rule — no permission prompt.

TOOL CALL 5 — Bash (re-capture full diff for reading):
  git diff <scope-args> > .er-diff-tmp

TOOL CALL 6 — Read .er-diff-tmp (the full diff):
  → This loads the ENTIRE diff into context. Do NOT read individual files.

(Optional) TOOL CALL 7 — Read .er-feedback.json if it exists

IN-CONTEXT ANALYSIS (zero tool calls):
  With the full diff in context, analyse ALL files in a single thinking pass:
  - For each file: risk level, risk_reason, summary, findings
  - Review order (group by concern, sort by risk)
  - Checklist (4-8 items linked to findings)
  - Summary (3-5 paragraph markdown)

  Finding schema:
  {
    "id": "f-<n>",
    "severity": "high|medium|low|info",
    "category": "security|logic|performance|correctness|error-handling|style|testing",
    "title": "Short title (max 60 chars)",
    "description": "What the issue is and why it matters",
    "hunk_index": <0-based index into the file's hunks>,
    "line_start": <optional new-side line number>,
    "line_end": <optional>,
    "suggestion": "What to do about it",
    "related_files": ["other/file.rs"],
    "responses": []
  }

TOOL CALLS 8-11 — Write all four output files (parallel):
  Write .er-review.json, .er-order.json, .er-checklist.json, .er-summary.md

TOOL CALL 12 — Bash (persist, one command):
  mkdir -p .er-reviews/<branch>/<commit>/ && cp .er-review.json .er-order.json .er-checklist.json .er-summary.md .er-reviews/<branch>/<commit>/

Print summary. (.er-diff-tmp left in place — gitignored, overwritten next run.)

Step 4: Incremental analysis (~12 tool calls total)

Uses the base review from Step 2 (already in context).

TOOL CALL 4 — Bash (per-file hashes via helper script):
  scripts/er-hash-files.sh <scope-args>

Compare each file hash against base review's file_hashes:
  - Hash matches → preserve findings as-is
  - Hash changed or new file → needs re-analysis
  - Files in base but not in new diff → drop

TOOL CALL 5 — Bash (re-capture full diff):
  git diff <scope-args> > .er-diff-tmp

TOOL CALL 6 — Read .er-diff-tmp (full diff into context)

(Optional) TOOL CALL 7 — Read .er-feedback.json if it exists

IN-CONTEXT ANALYSIS (zero tool calls):
  Analyse ONLY changed + new files. Merge with preserved findings.
  Regenerate order, checklist (preserve checked state for unchanged files), summary.

TOOL CALLS 8-11 — Write all four files (parallel)

TOOL CALL 12 — Bash (persist)

Print: "Incremental review: N files preserved, M files re-analyzed, K files new"
```

## Output schemas

### .er-review.json
```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "diff_scope": "branch|unstaged|staged",
  "commit_hash": "<short HEAD sha>",
  "created_at": "<ISO 8601>",
  "updated_at": "<ISO 8601>",
  "base_branch": "main",
  "head_branch": "feature/foo",
  "file_hashes": {
    "src/foo.rs": "<sha256 of file's diff chunk>"
  },
  "files": {
    "src/foo.rs": {
      "risk": "high",
      "risk_reason": "Modifies authentication logic",
      "summary": "Adds OAuth2 token refresh handling",
      "findings": [...]
    }
  }
}
```

New fields (additive, backward-compatible — TUI ignores unknown fields):
- `diff_scope` — which mode produced this review (branch/unstaged/staged)
- `commit_hash` — HEAD at time of review (persistence cache key)
- `updated_at` — distinguishes initial from incremental reviews
- `file_hashes` — per-file diff hashes for incremental comparison

### .er-order.json
```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "order": [
    {"path": "src/auth.rs", "reason": "Core auth change", "group": "auth"},
    {"path": "src/middleware.rs", "reason": "Uses new auth", "group": "auth"},
    {"path": "tests/auth_test.rs", "reason": "Test coverage", "group": "tests"}
  ],
  "groups": {
    "auth": {"label": "Authentication", "color": "red"},
    "tests": {"label": "Tests", "color": "green"}
  }
}
```

### .er-checklist.json
```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "items": [
    {
      "id": "c-1",
      "text": "Verify token refresh handles network errors",
      "category": "correctness",
      "checked": false,
      "related_findings": ["f-1", "f-3"],
      "related_files": ["src/auth.rs"]
    }
  ]
}
```

## Persistence model

Reviews are persisted by branch and commit hash:

```
.er-reviews/<branch>/<commit-hash>/
  .er-review.json
  .er-order.json
  .er-checklist.json
  .er-summary.md
```

**What the review covers vs the commit hash:**
- The review always covers the **full scope diff** — all changes for the selected scope.
- For branch scope: all changes from base branch to HEAD, across all commits on the branch.
- The commit hash is the **cache key** — "at HEAD=a1b2c3d, here's the review."
- When you add a new commit, the new review still covers the entire scope. The incremental logic compares the old diff against the new diff file-by-file. Files with identical diff chunks carry forward. Only modified files get re-analyzed.

**Cache lookup order:**
1. Exact match: `.er-reviews/<branch>/<current-commit>/` with matching diff_hash → fast restore
2. Stale match at current commit: use as incremental base
3. Most recent review in `.er-reviews/<branch>/`: use as incremental base
4. Nothing found: full analysis

## Large diff handling

For diffs >100KB or >20 files, apply these shortcuts to stay under 90 seconds:

- **Skip non-code files**: `.md`, `.json` config, `.toml`, `.lock`, `.gitignore` — mark as `risk: "info"` with a one-line summary, zero findings.
- **Trivially low-risk files**: re-exports (`mod.rs` with only `pub use`), test fixtures, generated files — `risk: "info"`, one-line summary, zero findings.
- **Cap findings**: max 3 per file, max 15 total across the review. Prioritize high/medium over low/info.
- **Don't read the full diff if >200KB**: use `git diff --stat` output + per-file hashes to identify which files changed, then read only high-risk files individually.
- **Incremental reviews should be fast**: if ≤3 files changed, the review should complete in <60 seconds. Don't regenerate the full summary/checklist/order — patch the existing ones.

## Guidelines

- Be specific. "Check error handling" is bad. "Handle the `None` case in `parse_token()` at line 42" is good.
- Pin findings to hunks using `hunk_index` (0-based). If a finding spans hunks, pick the most relevant one.
- Risk levels should be meaningful: `high` = likely bug or security issue, `medium` = code smell or missing edge case, `low` = style or minor improvement, `info` = observation.
- Keep titles under 60 characters — they render inline in the TUI.
- The `suggestion` field should be actionable — what to change, not just what's wrong.
- Don't generate more than 3-4 findings per file unless it's genuinely that problematic.
- The checklist should be things the reviewer should manually verify, not things Claude already checked.

## .gitignore

Add `.er-*` to the project's `.gitignore`. This single pattern covers all sidecar files:
- `.er-review.json`, `.er-order.json`, `.er-summary.md`, `.er-checklist.json`
- `.er-feedback.json`, `.er-feedback.prev.json`
- `.er-reviewed`
- `.er-reviews/` (persistence cache)
- `.er-diff-tmp` (temporary)
