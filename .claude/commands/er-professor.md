# er-professor

Teach what the current diff implements — learning insights, not a code review. Writes `.er/professor.json` only.

## Philosophy

Read [`../PROFESSOR_PHILOSOPHY.md`](../PROFESSOR_PHILOSOPHY.md) before running.

## Trigger

`/er-professor` or `/er-professor [scope] [base-branch] [optional focus text…]`

Scopes: `branch` (default), `unstaged`, `staged` — same as `/er-review`.

Example: `/er-professor branch main how does the auth flow work`

## Diff

Two-dot `git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, awk-annotate → `.er/diff-annotated`.

If `.er/review-files.txt` exists, read it and analyze **only** those paths.

## Professor lens

- Explain purpose, architecture, data flow, invariants, non-obvious design
- **Do not** flag bugs, security, or style (use `/er-review` for that)
- `severity`: always `info`
- `confidence`: `informational`
- `category`: `professor`
- Caps: ~3 insights per file, ~12 total
- Target under 3 minutes

When the user provides focus text (trailing args or in a prior prompt), prioritize that topic.

## Output

`mkdir -p .er` then write **only** `.er/professor.json`:

```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "diff_scope": "<scope>",
  "created_at": "<ISO8601>",
  "focus_prompt": "<user focus or empty>",
  "files": {
    "path/to/file.rs": {
      "findings": [
        {
          "id": "prof-1",
          "severity": "info",
          "category": "professor",
          "title": "Short concept label",
          "description": "Teaching explanation",
          "hunk_index": 0,
          "line_start": 42,
          "suggestion": "",
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

Do **not** write `review.json`, `order.json`, `checklist.json`, `summary.md`, or `.er/experts/*`.
