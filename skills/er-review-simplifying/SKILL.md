# er-review-simplifying

Specialized **Simplifying** review — readability and complexity. Writes `.er/experts/simplifying.json` only.

## Apply shared rules first

Follow [`../REVIEW_RULES.md`](../REVIEW_RULES.md) in full before this lens (diff, annotate, anchors, confidence, caps).

## Trigger

`/er-review-simplifying` or `/er-review-simplifying [scope] [base-branch]`

Scopes: `branch` (default), `unstaged`, `staged` — same as `/er-review`.

## Diff

Two-dot `git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, awk-annotate → `.er/diff-annotated`.

## Expert lens: Simplifying

Focus: code that is not easy to understand from reading the diff — complex control flow, heavy abstraction, implicit behavior, or patterns that need extra context.

For each finding:

- Propose a **concrete simplification** in `suggestion`, **or**
- If behavior must stay as-is, suggest a **short explanatory comment** (quote the comment text in `suggestion`)

- `category`: `simplifying`
- Finding ids: `simp-1`, `simp-2`, …
- Caps: **2 per file, 10 total**
- Target under 2 minutes

Do **not** flag naming, formatting, or style-only issues.

## Output

`mkdir -p .er/experts` then write **only** `.er/experts/simplifying.json`:

```json
{
  "version": 1,
  "expert_id": "simplifying",
  "diff_hash": "<sha256>",
  "diff_scope": "<scope>",
  "created_at": "<ISO8601>",
  "summary": "2–3 short markdown paragraphs (readability and complexity hotspots)",
  "files": {
    "path/to/file.rs": {
      "findings": [
        {
          "id": "simp-1",
          "severity": "medium",
          "category": "simplifying",
          "title": "Nested conditionals obscure branch",
          "description": "Three levels of if/else make the happy path hard to spot on first read.",
          "hunk_index": 0,
          "line_start": 42,
          "suggestion": "Extract early returns or a small helper so the main path reads top-to-bottom.",
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
