---
name: er-get-feedback
description: >
  Read open review questions, local notes, and AI findings on a PR from Easy Review
  managed storage. Use when the user asks what's open on a PR, wants feedback summary,
  or before responding to review threads. Accepts PR URL, worktree path, owner/repo, or branch.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — get feedback (`er-get-feedback`)

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

Read-only.

## Trigger phrases

- "What's open on this PR?" / "any feedback?" / "review questions"
- "Show findings" / "notes on PR #42"
- User gives PR URL, path, `owner/repo`, branch, or number

## Workflow

1. **`pr_resolve`** if needed, or pass `ref` directly.
2. **`pr_feedback_get`** with `{ "ref": "…", "include_resolved": false }` (true only if user wants history).
3. Optional **`pr_summarize`** for triage/review/tour sidecar summary.

## Output format

Group by type:

- **Questions** — things a human wants answered
- **Notes** — hand-off instructions for an agent
- **Findings** — AI review items (by severity if present)

Do **not** write replies in this skill — use **`er-respond`**.
