# Review Rules

Canonical operational rules for all `er` reviewers (general and specialized experts). Claude Code skills and Rust spawn prompts must stay aligned with this file.

See also: [`REVIEW_PHILOSOPHY.md`](REVIEW_PHILOSOPHY.md) for severity model and what-not-to-flag.

## Diff and hash

- **Two-dot diff only:** `git diff <base>` — never `main...HEAD` (staleness mismatch with `er`)
- Always **`--unified=20 --no-color --no-ext-diff`** (matches desktop engine)
- **Prepared-diff path (desktop):** hash `{output_dir}/diff-tmp`; agent must **not** re-run `git diff`
- **TUI / skill path:** `git diff … > .er/diff-tmp` (or `{output_dir}/diff-tmp`), then hash that file

## Annotate and anchor

1. Awk-annotate raw diff → `[h<N> L<M>]` tags on every content line
2. Read the annotated diff — 20 lines of context per hunk
3. **Findings only on `+` or `-` lines** — copy `hunk_index` / `line_start` from tags; never compute line numbers
4. Set `outside_diff: false` on review pass; drop unanchored findings

## Philosophy (embedded)

Severity mapping (from REVIEW_PHILOSOPHY):

| P-level | `severity` / risk |
|---------|-------------------|
| P0 | `high` |
| P1 | `medium` |
| P2 | `low` |
| observation | `info` |

**Do not flag:** naming, formatting, style preferences, import order, file moves without logic change, comment nits.

**Gate:** "Does this affect correctness, security, or reliability?" — if no, skip.

## Confidence and verification

- `confidence`: `confirmed` | `informational` | `tentative`
- `verification_plan` required for `tentative`
- `evidence`: cite files/ranges you read
- Budget: ~10 reads per finding (no global session cap); short-circuit obvious issues

## Finding caps

| Reviewer | Per file | Total |
|----------|----------|-------|
| General | 3–4 | 15 |
| Expert | 2 | 10 |

## Allowed categories

- **General:** `security`, `logic`, `performance`, `correctness`, `error-handling`, `testing`, `api` — **no `style`**
- **Expert:** `category` = expert id (`security`, `patterns`, …); only report issues in that lens

## Speed

- General: target under 3 minutes
- Expert: target under 2 minutes
