# er-review-patterns

Specialized **Patterns** review — consistency with existing code. Writes `.er/experts/patterns.json` only.

## Apply shared rules first

Follow [`../REVIEW_RULES.md`](../REVIEW_RULES.md) in full before this lens.

## Trigger

`/er-review-patterns [scope] [base-branch]`

## Diff

`git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, annotate.

## Expert lens: Patterns

1. Identify symbols/patterns introduced or changed in the diff
2. `grep` / read **2–5** similar usages (same directory or module first)
3. Flag only deviations that affect **correctness or maintainability** (no naming/style nits)
4. Cite established patterns in `evidence` (file, line range, note)

- `category`: `patterns`; ids `pat-1`, …; caps **2/file, 10 total**

## Output

Write **only** `.er/experts/patterns.json` with full `Finding` schema.
