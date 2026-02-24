# er-review

Quick AI code review for the current git diff, producing `.er-*` sidecar files that the `er` TUI reads.

## Trigger

Run as `/er-review` or `/er-review <base-branch>`.

## Diff source — CRITICAL

Both `/er-review` and `/review-pr` MUST use the same diff command that `er` uses internally:

```
git diff <base> --unified=3 --no-color --no-ext-diff
```

**This is a two-dot diff** (`git diff main`), comparing the base branch to the **working tree**.
Do NOT use three-dot (`git diff main...HEAD`) — that compares to HEAD only and produces a
different hash, causing `er` to show the review as stale.

## What it does

1. Reads the working tree diff (matching `er`'s internal diff command)
2. Computes a SHA-256 `diff_hash` of the raw diff for staleness detection
3. Analyses every changed file and hunk (single-pass, no agents)
4. Writes four files to the repo root:
   - `.er-review.json` — per-file risk levels, summaries, and findings pinned to hunks
   - `.er-order.json` — suggested review order grouped by logical concern
   - `.er-checklist.json` — actionable review checklist items
   - `.er-summary.md` — human-readable overall summary

## Feedback-aware mode

Before generating, check if `.er-feedback.json` exists and its `diff_hash` matches the current diff. If it does, read the human comments and:
- Address each comment in the relevant finding's `responses` array
- Add new findings if a comment reveals something you missed
- Archive the old feedback to `.er-feedback.prev.json`

If `.er-feedback.json` exists but its `diff_hash` doesn't match, ignore it (it's stale).

## Step-by-step

```
1. Determine the base branch:
   - If an argument is provided, use it
   - Otherwise: detect main or master

2. Get the raw diff (MUST match er's internal command):
   git diff <base> --unified=3 --no-color --no-ext-diff

3. Compute diff_hash:
   Save the raw diff to a temp file, then: shasum -a 256 <file>
   (Do NOT pipe through shasum — the safe-bash hook blocks piped hash commands.)

4. Check for existing feedback:
   - Read .er-feedback.json if it exists
   - If diff_hash matches, incorporate comments into analysis

5. Analyse each file:
   - Assign risk level: high | medium | low | info
   - Write a 1-line risk_reason
   - Write a 1-line summary
   - For each concerning hunk, create a Finding:
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

6. Determine review order:
   - Group files by logical concern (e.g., "data layer", "API", "tests")
   - Order groups by risk (highest first)

7. Generate checklist:
   - 4-8 items based on the actual findings
   - Link each item to relevant findings via related_findings
   - Categories: correctness, security, testing, compatibility, performance

8. Write summary:
   - 3-5 paragraph markdown overview
   - Overall risk assessment
   - Key concerns and what to focus on

9. Write all four files to the repo root
```

## Output schemas

### .er-review.json
```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "created_at": "<ISO 8601>",
  "base_branch": "main",
  "head_branch": "feature/foo",
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

## Guidelines

- Be specific. "Check error handling" is bad. "Handle the `None` case in `parse_token()` at line 42" is good.
- Pin findings to hunks using `hunk_index` (0-based). If a finding spans hunks, pick the most relevant one.
- Risk levels should be meaningful: `high` = likely bug or security issue, `medium` = code smell or missing edge case, `low` = style or minor improvement, `info` = observation.
- Keep titles under 60 characters — they render inline in the TUI.
- The `suggestion` field should be actionable — what to change, not just what's wrong.
- Don't generate more than 3-4 findings per file unless it's genuinely that problematic.
- The checklist should be things the reviewer should manually verify, not things Claude already checked.

## .gitignore

These files should be gitignored:
```
.er-review.json
.er-order.json
.er-summary.md
.er-checklist.json
.er-feedback.json
.er-feedback.prev.json
```
