---
name: er-respond
description: >
  Reply to review questions, local notes, or AI findings on a PR via Easy Review MCP.
  Use when the user wants to answer a question, validate a finding, or respond on a PR
  thread. Accepts PR URL, worktree path, owner/repo, or branch.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — respond (`er-respond`)

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

Mutating.

## Trigger phrases

- "Answer this question" / "reply to the note" / "validate this finding"
- "Respond on PR #42" with question/note/finding id

## Workflow

1. **`pr_feedback_get`** first (unless user gave `type` + `id` explicitly).
2. **`pr_feedback_reply`**:

```json
{
  "ref": "…",
  "type": "question",
  "id": "q-…",
  "text": "…"
}
```

`type`: `question` | `note` | `finding`

3. **`pr_feedback_get`** again to confirm.

## Rules

- Route question → `question`, note → `note`, finding → `finding`.
- Do not resolve or delete items (not supported via MCP yet).
- Do not invent ids — use ids from `pr_feedback_get`.
