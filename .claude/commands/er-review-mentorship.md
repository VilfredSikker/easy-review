# er-review-mentorship

Specialized **Mentorship** review — highlight exemplary patterns to foster. Writes `.er/experts/mentorship.json` only.

## Apply shared rules first

Follow [`../REVIEW_RULES.md`](../REVIEW_RULES.md) for diff, annotate, and anchors. This lens overrides severity: findings are **positive highlights**, not defects.

## Trigger

`/er-review-mentorship` or `/er-review-mentorship [scope] [base-branch]`

Scopes: `branch` (default), `unstaged`, `staged` — same as `/er-review`.

## Diff

Two-dot `git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, awk-annotate → `.er/diff-annotated`.

## Expert lens: Mentorship

Focus: **good** code in the diff — patterns and quality the team should see more of.

- Explain what is exemplary and **why** it works well
- Use `suggestion` to suggest where to replicate or extend the pattern — not bug fixes
- Do **not** report problems (security, logic, style, etc.)

- `category`: `mentorship`
- Finding ids: `ment-1`, `ment-2`, …
- `severity`: always `info`
- `confidence`: `informational`
- Caps: **2 per file, 10 total**
- Target under 2 minutes

## Output

`mkdir -p .er/experts` then write **only** `.er/experts/mentorship.json`:

```json
{
  "version": 1,
  "expert_id": "mentorship",
  "diff_hash": "<sha256>",
  "diff_scope": "<scope>",
  "created_at": "<ISO8601>",
  "files": {
    "path/to/file.rs": {
      "findings": [
        {
          "id": "ment-1",
          "severity": "info",
          "category": "mentorship",
          "title": "Clear error mapping at boundary",
          "description": "Domain errors are converted once at the API layer with stable codes — easy for callers and reviewers to reason about.",
          "hunk_index": 0,
          "line_start": 42,
          "suggestion": "Reuse this boundary pattern in other handlers that still return raw internal errors.",
          "related_files": [],
          "outside_diff": false,
          "confidence": "informational",
          "verification_plan": "",
          "evidence": [],
          "responses": [],
          "resolved": false,
          "resolved_note": "",
          "resolved_at": ""
        }
      ]
    }
  }
}
```

Do **not** write `review.json`, `order.json`, `checklist.json`, or `summary.md`.
