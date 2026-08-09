---
name: er-saved
description: >
  Pin, unpin, or list saved PRs and uploaded Easy Review artifacts in Desktop Saved
  and managed storage. Use when the user wants to pin a reviewed PR, list saved work,
  or find PRs with triage/review/tour sidecars.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — saved PRs (`er-saved`)

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

- "Pin this PR" / "save to Desktop"
- "List saved PRs" / "what have I reviewed?"
- "Show uploaded artifacts"

## MCP: `pr_saved`

| Action | Call |
|--------|------|
| Pin | `{ "action": "pin", "ref": "…" }` |
| Unpin | `{ "action": "unpin", "ref": "…" }` |
| Saved only | `{ "action": "list", "source": "pinned" }` |
| Artifacts scan | `{ "action": "list", "source": "artifacts" }` |
| Both | `{ "action": "list", "source": "all" }` |

Optional `kinds: ["triage", "tour"]` on list to filter artifacts.

Only pin when the user asks or after a successful **`er-review`** they asked to save.
