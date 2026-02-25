# er-refresh

Lightweight re-evaluation of existing review findings after code changes. Validates whether fixes actually addressed the issues — removes resolved findings, keeps valid ones.

## Trigger

Run as `/er-refresh`.

## When to use

After running `/er-review` and making fixes. Instead of re-running the full review, this validates existing findings against the updated diff. Much faster since it only checks changed files and doesn't discover new issues.

## What it does

1. Reads the existing `.er-review.json`
2. Compares per-file diff hashes to identify which files changed
3. For changed files: re-evaluates each finding against the new code
4. Removes findings that are resolved, updates those that persist
5. Writes updated `.er-*` files with the current `diff_hash`

## Speed budget

**Target: ≤8 tool calls total, ≤45 seconds.** This is a validation pass, not discovery.

### Permission & hook constraints

All Bash commands MUST start with an allowed command.
Allowed first-words: `git`, `shasum`, `mkdir`, `cp`, `scripts/er-*`
NOT allowed as first word: `for`, `rm`, `while`, `bash`, `sh`

## Step-by-step

```
Step 1: Read existing review (1 tool call)

TOOL CALL 1 — Read .er-review.json:
  → If missing: print "No review found. Run /er-review first.", DONE.
  → Extract: diff_hash, diff_scope, base_branch, head_branch, file_hashes, files

Step 2: Compute current state (1-2 tool calls)

Determine scope args from the review's diff_scope + base_branch:
- "branch" → <base_branch> --unified=3 --no-color --no-ext-diff
- "unstaged" → --unified=3 --no-color --no-ext-diff
- "staged" → --staged --unified=3 --no-color --no-ext-diff

If diff_scope is missing, default to "branch".

TOOL CALL 2 — Bash (hash + per-file hashes):
  For branch scope:
    scripts/er-freshness-check.sh <base_branch>
  For unstaged/staged:
    git diff <scope-args> > .er-diff-tmp && shasum -a 256 .er-diff-tmp
  → Get current diff_hash

TOOL CALL 3 — Bash (per-file hashes):
  scripts/er-hash-files.sh <scope-args>
  → Compare each file hash against review.file_hashes

  → If current diff_hash matches review.diff_hash:
    Print "Review is current — no changes detected.", DONE.

  → If ALL per-file hashes match:
    Print "Review is current — no file-level changes.", DONE.

  → Identify changed files (hash mismatch), unchanged files (hash match),
    removed files (in review but not in diff), new files (in diff but not in review).

Step 3: Load diffs for changed files (1-2 tool calls)

TOOL CALL 4 — Bash: capture full diff
  git diff <scope-args> > .er-diff-tmp

TOOL CALL 5 — Read .er-diff-tmp
  → Load into context

Step 4: In-context validation (zero tool calls)

This is the core analysis. For each CHANGED file, examine its existing findings
against the new diff. For each finding, determine one of:

  - RESOLVED: The code was changed and the issue described in the finding is fixed.
    The specific problem (missing error handling, incorrect logic, security issue, etc.)
    is no longer present in the new code.
    → Action: Remove the finding.

  - PERSISTS: The code changed but the issue described in the finding is still present.
    The fix didn't address this particular concern, or introduced the same pattern elsewhere.
    → Action: Keep the finding. Update hunk_index/line_start/line_end if the code moved.

  - SHIFTED: The surrounding code changed (e.g., lines added above) but the finding's
    target code is functionally identical. Only the line numbers changed.
    → Action: Keep the finding, update hunk_index/line_start/line_end to new positions.

Be precise about RESOLVED vs PERSISTS. A finding is only RESOLVED if the specific
issue it describes is actually fixed. Changing the code around it doesn't resolve it.

For UNCHANGED files: preserve all findings exactly as-is (zero analysis needed).
For REMOVED files: drop all their findings (the file is no longer in the diff).
For NEW files: no findings to validate (they weren't in the original review).

After validation, produce:
- Updated files map (removed resolved findings, updated positions)
- Updated file_hashes (from Step 2 per-file hashes)
- Brief refresh summary

Regenerate .er-order.json to remove entries for removed files.
Regenerate .er-checklist.json: uncheck items whose related_findings were all resolved,
preserve checked state for items with persisting findings.
Regenerate .er-summary.md: append a "Refresh" section noting what changed.

Step 5: Write updated files (3-4 tool calls)

TOOL CALLS 6-9 — Write updated files in parallel:
  - .er-review.json (with new diff_hash, updated file_hashes, updated_at, preserved version)
  - .er-order.json (updated)
  - .er-checklist.json (updated)
  - .er-summary.md (updated with refresh note)

TOOL CALL 10 — Bash: persist to cache
  mkdir -p .er-reviews/<branch>/<commit>/ && cp .er-review.json .er-order.json .er-checklist.json .er-summary.md .er-reviews/<branch>/<commit>/

Print summary:
  "Refresh complete: N findings resolved, M persisting, K shifted"
  List each resolved finding briefly: "  ✓ [file] finding title"
  List persisting findings: "  → [file] finding title"
```

## Output schema

Same as `/er-review`. The `.er-review.json` output follows the exact same schema —
only the content changes (findings removed/updated, hashes updated).

Key fields to update in `.er-review.json`:
- `diff_hash` → current diff hash
- `file_hashes` → current per-file hashes
- `updated_at` → current ISO 8601 timestamp
- `files` → updated findings (resolved ones removed)

## Guidelines

- Be conservative: when in doubt whether a finding is resolved, keep it (PERSISTS).
- Don't discover new issues — that's what `/er-review` is for. This is validation only.
- If more than 50% of files changed, suggest running `/er-review` instead for a fresh analysis.
- Keep the summary update brief — one paragraph noting what was refreshed.
- If no findings were resolved after checking, say so: "All N findings still apply."

## .gitignore

No new files — uses the same `.er-*` files as `/er-review`.
