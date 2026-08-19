---
name: er-queue
description: >
  Find PRs to review next — priority queue, review debt, blocked, stale, or cross-repo.
  Use when the user asks what to review, review debt, blocked PRs, stale PRs, or
  priority across Easy Review projects.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — review queue (`er-queue`)

- MCP server **`easy-review`** connected (`npx -y easy-review-mcp` or `er-mcp` on PATH).
- Authenticated **`gh`** (`gh auth status`).
- Optional: Easy Review projects in `~/.config/er/projects.json` (Desktop writes this).

Managed PR storage (read `diff-tmp` only; never write here directly):

- macOS: `~/Library/Application Support/easy-review/repos/<owner-repo>/prs/pr-<N>/`
- Linux: `~/.local/share/easy-review/repos/<owner-repo>/prs/pr-<N>/`

Always upload sidecars through **`pr_upload`**, not by writing into the managed directory.

Pass the user's target as **`ref`** on any PR-scoped MCP tool, or call **`pr_resolve`** first.

## Accepted `ref` forms

| User types | Example | Resolves via |
|------------|---------|--------------|
| PR URL | `https://github.com/o/r/pull/42` | URL parse |
| Worktree path | `/Users/me/Projects/foo` | git toplevel → open PR for current branch |
| `owner/repo#N` | `acme/widgets#42` | Explicit PR number |
| `owner/repo` | `vilfred/ai-report-builder` | ER project with that remote → open PR in that checkout |
| Branch name | `feature/auth` | `gh pr list --head` in project/cwd repo |
| Bare number | `42` | Active ER project or `repo=` |

## Skill workflow

1. If the user gave a ref (URL, path, slug, branch, number), pass it as `ref` on the tool call.
2. If ambiguous, call **`pr_resolve`** once and use the returned `repo`, `number`, `pr_url`, `bucket_path`.
3. Do not guess a different PR than `pr_resolve` returns.

## Examples

```text
er-review https://github.com/acme/widgets/pull/42
er-review /Users/me/Projects/Discovery/discovery
er-review vilfred/ai-report-builder
er-get-feedback feature/my-branch
```

## Errors

- **No open PR for branch** — ask for a PR URL or `owner/repo#N`.
- **No ER project for owner/repo** — pass a worktree path or configure the project in Desktop.
- **No PR on current branch** — checkout the PR branch or pass an explicit URL.


## Trigger phrases

- "What should I review?" / "priority PRs" / "review queue"
- "My review debt" / "PRs waiting on me"
- "Blocked PRs" / "failing CI" / "stale PRs"
- "Across all my projects" / "cross-repo queue"

## MCP: `prs_query`

| Intent | Call |
|--------|------|
| Priority (default) | `{ "sort": "priority", "limit": 10 }` |
| Review debt | `{ "filter": "review_debt" }` |
| Stale | `{ "filter": "stale", "stale_days": 14 }` |
| Blocked | `{ "filter": "blocked", "scan_limit": 15 }` |
| Failing CI | `{ "filter": "failing_ci" }` |
| Cross-repo | `{ "cross_repo": true, "limit": 10 }` |
| Ready to review | `{ "filter": "ready" }` |

Optional: `production_lines: true` for production-only line enrichment (slower).

Scoped to one repo: pass `ref` as `owner/repo` on `prs_query` (or `repo` + `project_id`).

## Output

Present a compact table: `#`, title, score/reasons, size, status. Offer **`er-review`** on the user's pick — do not auto-review.
