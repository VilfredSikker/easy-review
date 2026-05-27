# er-review-api

Specialized **API / contracts** review. Writes `.er/experts/api.json` only.

## Apply shared rules first

Follow [`../REVIEW_RULES.md`](../REVIEW_RULES.md) in full.

## Trigger

`/er-review-api [scope] [base-branch]`

## Diff

`git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash, annotate.

## Expert lens: API / contracts

Breaking changes, public surface, semver impact.

- `category`: `api`; ids `api-1`, …; caps **2/file, 10 total**

## Output

Write **only** `.er/experts/api.json` with top-level `summary`: 2–3 markdown paragraphs on API/contract impact (breaking changes, public surface, semver).
