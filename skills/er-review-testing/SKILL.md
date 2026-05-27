# er-review-testing

Specialized **Testing** review. Writes `.er/experts/testing.json` only.

## Apply shared rules first

Follow [`../REVIEW_RULES.md`](../REVIEW_RULES.md) and [`../REVIEW_PHILOSOPHY.md`](../REVIEW_PHILOSOPHY.md) test-quality rules.

## Trigger

`/er-review-testing [scope] [base-branch]`

## Diff

`git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, annotate.

## Expert lens: Testing

Assertion quality, missing negative cases, shallow existence-only tests.

- `category`: `testing`; ids `tst-1`, …; caps **2/file, 10 total**

## Output

Write **only** `.er/experts/testing.json` (include a lens-specific `summary` field — test coverage/quality, not a general changelog).
