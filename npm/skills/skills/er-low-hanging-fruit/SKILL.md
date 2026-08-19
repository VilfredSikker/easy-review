---
name: er-low-hanging-fruit
description: >
  Find the smallest / quickest PRs to review using production-only line counts.
  Use when the user asks for low-hanging fruit, smallest PRs, quick wins, or wants
  to compare PR sizes. Optional ref scopes to one repo.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — low-hanging fruit (`er-low-hanging-fruit`)

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

- "Low-hanging fruit" / "smallest PRs" / "quick wins"
- "Compare PR sizes" / "which PR is smallest?"

## MCP calls

**Ranked smallest (default):**

```json
{
  "sort": "smallest",
  "production_lines": true,
  "limit": 5,
  "repo": "owner/name"
}
```

Use `ref: "owner/repo"` or `projects_list` when repo omitted.

**Compare specific PR numbers:**

```json
{ "ref": "owner/repo", "numbers": [12, 15, 18] }
```

via **`pr_stats`** with `numbers` array.

**Single PR depth:**

```json
{ "ref": "https://github.com/o/r/pull/42", "include_hotspots": true }
```

## Output

Table: `#`, title, production lines, total lines, review state. Offer **`er-review`** on pick — do not auto-review.
