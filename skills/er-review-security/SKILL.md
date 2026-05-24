# er-review-security

Specialized **Security** review for the current git diff. Writes `.er/experts/security.json` only.

## Apply shared rules first

Read and follow [`../REVIEW_RULES.md`](../REVIEW_RULES.md) in full before this lens (diff, annotate, anchors, confidence, caps).

## Trigger

`/er-review-security` or `/er-review-security [scope] [base-branch]`

Scopes: `branch` (default), `unstaged`, `staged` — same as `/er-review`.

## Diff

Two-dot `git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, awk-annotate → `.er/diff-annotated`.

## Expert lens: Security

Focus: AuthZ/authN, injection, secrets in diff, unsafe defaults, trust boundaries.

- `category`: `security`
- Finding ids: `sec-1`, `sec-2`, …
- Caps: **2 per file, 10 total**
- Target under 2 minutes

## Output

`mkdir -p .er/experts` then write **only** `.er/experts/security.json`:

```json
{
  "version": 1,
  "expert_id": "security",
  "diff_hash": "<sha256>",
  "diff_scope": "<scope>",
  "created_at": "<ISO8601>",
  "files": {
    "path/to/file.rs": {
      "findings": [
        {
          "id": "sec-1",
          "severity": "high",
          "category": "security",
          "title": "Short title",
          "description": "What and why",
          "hunk_index": 0,
          "line_start": 42,
          "suggestion": "Fix",
          "related_files": [],
          "outside_diff": false,
          "confidence": "confirmed",
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
