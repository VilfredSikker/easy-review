# er-review-performance

Specialized **Performance** review. Writes `.er/experts/performance.json` only.

## Apply shared rules first

Follow [`../REVIEW_RULES.md`](../REVIEW_RULES.md) in full before this lens.

## Trigger

`/er-review-performance [scope] [base-branch]` — scopes: `branch`, `unstaged`, `staged`.

## Diff

`git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, annotate → `.er/diff-annotated`.

## Expert lens: Performance

Hot paths, allocations, blocking I/O, unnecessary work in the diff.

- `category`: `performance`; ids `perf-1`, …; caps **2/file, 10 total**; target **&lt;2 min**

## Output

Write **only** `.er/experts/performance.json` (same `Finding` schema as `/er-review`, including `confidence`, `evidence`, `outside_diff`). Include top-level `summary`: 2–3 markdown paragraphs on performance impact (hot paths, allocations, blocking I/O) — not a general changelog.
