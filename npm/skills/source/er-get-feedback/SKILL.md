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

Read-only. See [`../_shared/REF_RESOLUTION.md`](../_shared/REF_RESOLUTION.md).

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
