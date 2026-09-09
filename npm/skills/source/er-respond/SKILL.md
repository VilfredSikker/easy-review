---
name: er-respond
description: >
  Answer review questions and implement actionable local notes on a PR, then reply via
  Easy Review MCP. Use when the user wants to answer a question, act on a note, validate
  a finding, or respond on a PR thread. Accepts PR URL, worktree path, owner/repo, or
  branch.
metadata:
  author: easy-review
  version: "0.2.0"
---

# Easy Review — respond (`er-respond`)

Mutating. Questions get an answer. Notes are actionable implementation requests.
See [`../_shared/REF_RESOLUTION.md`](../_shared/REF_RESOLUTION.md).

## Handling rules

- Question: Answer and discuss only. Do not edit code until the user approves a change.
- Note: Apply the requested change unless the note is stale.
- Finding: Validate against the current code. Fix only when explicitly requested.
- Resolved: Take no action.
- Stale: Reason about whether it still applies and where, then ask before acting on it.

> A question remains discussion-only even when it proposes or recommends a code change.

Resolved items are closed out. Skip them entirely — no reply, no change — unless
the user names one. `pr_feedback_get` omits them by default; they only appear
with `include_resolved: true`, or as `[resolved]` in a markdown export.

Staleness does not travel over MCP — `pr_feedback_get` returns each item's
`line_content` and the bucket's `diff_hash`, never a `stale` flag. Treat an item
as stale when its `line_content` no longer matches the code at its anchor. A
markdown export marks stale items `[stale]`; Desktop and the TUI dim them. On a
stale item, work out where it now applies and whether it still holds, say so, and
wait for the user to confirm before changing anything.

These rules are also emitted as a preamble in every Easy Review markdown export,
so a pasted export carries them without this skill installed.

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
2. Classify the selected item and act on it per **Handling rules** above. What
   that means in practice:
   - **Question** — answer from the current code and review context, citing
     files and lines. Recommend a change in the reply when one is warranted,
     then wait for the user.
   - **Note** — inspect the target worktree, make the requested change, and run
     the narrowest relevant checks. Preserve unrelated work. If the note is
     ambiguous or requires a broader change than the user authorized, stop and
     ask for direction.
   - **Finding** — reply with whether it still holds against the current code,
     and the evidence for that verdict.
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
  A stale note is the exception — say what it now refers to and wait for the
  user to confirm before changing anything.
- Do not resolve or delete items (not supported via MCP yet).
- Do not invent ids — use ids from `pr_feedback_get`.
