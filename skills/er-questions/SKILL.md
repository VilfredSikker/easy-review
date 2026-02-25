# er-questions

Read human feedback from `.er-feedback.json` and respond to each comment by updating `.er-review.json` with threaded responses.

## Trigger

Run as `/er-questions`.

## What it does

1. Reads `.er-feedback.json` — human comments added via the `er` TUI (press `c` on a hunk)
2. Reads `.er-review.json` — the current AI review
3. Validates both have matching `diff_hash` (if not, warn and abort)
4. For each unresolved feedback comment:
   - Finds the related finding (via `in_reply_to` field)
   - Reads the relevant code context from the diff
   - Writes a thoughtful response in the finding's `responses` array
   - If the comment reveals a new issue, adds a new finding
   - If the comment resolves a concern, notes this in the response
5. Writes the updated `.er-review.json`
6. Archives the processed feedback to `.er-feedback.prev.json`

## Speed budget

**Target: ~6 tool calls total.**

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.
Use `scripts/er-freshness-check.sh <base>` for base validation + diff + hash.

## Step-by-step

```
TOOL CALL 1 — Read .er-feedback.json
  - If it doesn't exist, print "No feedback to process" and exit
  - Parse JSON: extract comments array and diff_hash

TOOL CALL 2 — Read .er-review.json
  - If it doesn't exist, print "No review to update — run /er-review first" and exit
  - Extract base_branch (used in next step)

TOOL CALL 3 — Bash (validate freshness):
  scripts/er-freshness-check.sh <base_branch>
  → Output: "ok", hash line, commit hash
  - Compare hash against feedback.diff_hash and review.diff_hash
  - If feedback stale: warn "Feedback is stale (diff changed). Skipping." and exit
  - If review stale: warn "Review is stale. Run /er-review first." and exit

TOOL CALL 4 — Read .er-diff-tmp (full diff into context)
  - This is ALL the code context needed. Do NOT read individual source files per comment.

IN-CONTEXT (zero tool calls) — Process all comments from the diff:
  For each comment in feedback.comments where resolved == false:

  a. If comment.in_reply_to is set:
     - Locate the hunk for comment.file / comment.hunk_index in the diff already in context
     - Find that finding in review.files[comment.file].findings
     - Add a response to finding.responses:
       {
         "id": "r-<n>",
         "in_reply_to": "<comment.id>",
         "timestamp": "<ISO 8601>",
         "text": "<thoughtful response referencing actual code from the diff>",
         "new_findings": []
       }

  b. If the comment is a general question (no in_reply_to):
     - Create a new finding if warranted
     - Or add a response to the most relevant existing finding

  c. If the comment says "resolved", "ok", "fixed", etc:
     - Note in response that the concern is addressed
     - Mark the comment's finding with a resolution note

TOOL CALL 5 — Write updated .er-review.json (same diff_hash, updated findings/responses)

TOOL CALL 6 — Bash: cp .er-feedback.json .er-feedback.prev.json

Print summary: "Processed N comments. Added M responses, K new findings."
```

## Response quality guidelines

- Actually read the code the human is asking about. Don't give generic answers.
- If they disagree with a finding, engage with their reasoning. Sometimes they're right.
- If they ask "why?", explain the actual risk with specifics from their code.
- If they say "I'll fix this" or mark something resolved, acknowledge it and move on.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive about your original review. If you were wrong, say so.

## Example feedback → response flow

**Human comment** (in .er-feedback.json):
```json
{
  "id": "fb-1",
  "file": "src/auth.rs",
  "hunk_index": 0,
  "comment": "This is intentional — we unwrap here because the config is validated at startup",
  "in_reply_to": "f-1"
}
```

**AI response** (added to finding f-1 in .er-review.json):
```json
{
  "id": "r-1",
  "in_reply_to": "fb-1",
  "timestamp": "2025-01-15T10:30:00Z",
  "text": "Makes sense — if config validation at startup guarantees this value exists, the unwrap is safe. I'd suggest adding a comment at the unwrap site noting the startup invariant for future readers.",
  "new_findings": []
}
```
