---
name: er-respond
description: >
  Answer review questions and implement actionable local notes on a PR, then reply via
  Easy Review MCP. Use when the user wants to answer a question, act on a note, validate
  a finding, or respond on a PR thread. Accepts PR URL, worktree path, owner/repo, or
  branch.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — respond (`er-respond`)

Mutating. Questions get an answer. Notes are actionable implementation requests.
See [`../_shared/REF_RESOLUTION.md`](../_shared/REF_RESOLUTION.md).

## Local branch feedback

`local` is a special target for the current checked-out branch. It reads and
writes the local branch bucket, not the PR bucket.

When the user runs `er-respond local`:

1. Call `pr_feedback_get` with `{ "bucket": "local", "include_resolved": false }`.
2. The MCP tool resolves the current repository and checked-out branch. Do not
   pass `ref`, `repo`, `project_id`, or `number` with `bucket: "local"`.
3. When replying, pass the same `bucket: "local"` to `pr_feedback_reply`.

The local bucket contains the questions and notes created while reviewing the
local branch diff. A normal target such as a PR URL continues to use the PR
bucket and should not be mixed with the local workflow.

## Trigger phrases

- "Answer this question" / "reply to the note" / "validate this finding"
- "Implement these review notes" / "address the notes"
- "Respond on PR #42" with question/note/finding id

## Workflow

1. **`pr_feedback_get`** first (unless user gave `type` + `id` explicitly).
2. Classify the selected item and act on it:
   - **Question** — answer it from the current code and review context. Do not
     edit the worktree unless the user separately asks for a change.
   - **Note** — treat it as an implementation request. Inspect the target
     worktree, make the requested change, and run the narrowest relevant checks.
     Preserve unrelated work. If the note is ambiguous or requires a broader
     change than the user authorized, stop and ask for direction.
   - **Finding** — respond with whether the finding is valid. Do not fix it
     unless the user explicitly requests a fix or a review-fix workflow includes
     it.
3. **`pr_feedback_reply`** after acting:

```json
{
  "ref": "…",
  "type": "question",
  "id": "q-…",
  "text": "…"
}
```

`type`: `question` | `note` | `finding`

4. **`pr_feedback_get`** again to confirm.

## Rules

- Route question → `question`, note → `note`, finding → `finding`.
- A note is not complete when it has only been acknowledged. Implement the
  requested change before replying, then summarize the change and validation in
  the reply. Do not claim completion if implementation or validation is blocked.
- Do not resolve or delete items (not supported via MCP yet).
- Do not invent ids — use ids from `pr_feedback_get`.
