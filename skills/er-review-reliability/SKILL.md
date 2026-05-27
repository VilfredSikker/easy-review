# er-review-reliability

Specialized **Reliability** review. Writes `.er/experts/reliability.json` only.

## Apply shared rules first

Follow [`../REVIEW_RULES.md`](../REVIEW_RULES.md) in full before this lens.

## Trigger

`/er-review-reliability [scope] [base-branch]`

## Diff

`git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, annotate.

## Expert lens: Reliability

Error handling, retries, timeouts, resource cleanup.

- `category`: `reliability`; ids `rel-1`, …; caps **2/file, 10 total**

## Output

Write **only** `.er/experts/reliability.json` with full `Finding` objects (`confidence`, `evidence`, etc.) and top-level `summary`: 2–3 markdown paragraphs on reliability (error handling, retries, timeouts, resource cleanup).
