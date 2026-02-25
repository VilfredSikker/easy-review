# er-questions

Read personal review questions from `.er-questions.json` and respond to each by updating `.er-review.json` with threaded responses.

## Trigger

Run as `/er-questions`.

## What it does

1. Reads `.er-questions.json` — personal review questions added via the `er` TUI (press `q` on a line or `Q` on a hunk)
2. Reads `.er-review.json` — the current AI review
3. Validates both have matching `diff_hash` (if not, warn and abort)
4. For each unresolved question:
   - Finds the related finding (via file/hunk context)
   - Reads the relevant code context from the diff
   - Writes a thoughtful response in the finding's `responses` array
   - If the question reveals a new issue, adds a new finding
   - If the question resolves a concern, notes this in the response
5. Writes the updated `.er-review.json`
6. Archives the processed questions to `.er-questions.prev.json`

Note: Questions are personal/private — they are NOT synced to GitHub. Use `c`/`C` in `er` for GitHub PR comments instead.

## Speed budget

**Target: ~6 tool calls total.**

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.
Use `scripts/er-freshness-check.sh <base>` for base validation + diff + hash.

## Step-by-step

```
TOOL CALL 1 — Read .er-questions.json
  - If it doesn't exist, print "No questions to process" and exit
  - Parse JSON: extract questions array and diff_hash

TOOL CALL 2 — Read .er-review.json
  - If it doesn't exist, print "No review to update — run /er-review first" and exit
  - Extract base_branch (used in next step)

TOOL CALL 3 — Bash (validate freshness):
  scripts/er-freshness-check.sh <base_branch>
  → Output: "ok", hash line, commit hash
  - Compare hash against questions.diff_hash and review.diff_hash
  - If questions stale: warn "Questions are stale (diff changed). Skipping." and exit
  - If review stale: warn "Review is stale. Run /er-review first." and exit

TOOL CALL 4 — Read .er-diff-tmp (full diff into context)
  - This is ALL the code context needed. Do NOT read individual source files per comment.

IN-CONTEXT (zero tool calls) — Process all questions from the diff:
  For each question in questions.questions where resolved == false:

  a. Locate the hunk for question.file / question.hunk_index in the diff already in context
  b. Find the most relevant finding in review.files[question.file].findings
  c. Add a response to finding.responses:
     {
       "id": "r-<n>",
       "in_reply_to": "<question.id>",
       "timestamp": "<ISO 8601>",
       "text": "<thoughtful response referencing actual code from the diff>",
       "new_findings": []
     }
  d. If the question reveals a new issue:
     - Create a new finding
  e. If the question says "resolved", "ok", "fixed", etc:
     - Note in response that the concern is addressed

TOOL CALL 5 — Write updated .er-review.json (same diff_hash, updated findings/responses)

TOOL CALL 6 — Bash: cp .er-questions.json .er-questions.prev.json

Print summary: "Processed N questions. Added M responses, K new findings."
```

## Response quality guidelines

- Actually read the code the human is asking about. Don't give generic answers.
- If they disagree with a finding, engage with their reasoning. Sometimes they're right.
- If they ask "why?", explain the actual risk with specifics from their code.
- If they say "I'll fix this" or mark something resolved, acknowledge it and move on.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive about your original review. If you were wrong, say so.

## Example feedback → response flow

**Human question** (in .er-questions.json):
```json
{
  "id": "q-1",
  "file": "src/auth.rs",
  "hunk_index": 0,
  "text": "Why is this unwrap safe? Couldn't config be missing?"
}
```

**AI response** (added to most relevant finding in .er-review.json):
```json
{
  "id": "r-1",
  "in_reply_to": "q-1",
  "timestamp": "2025-01-15T10:30:00Z",
  "text": "Good question. The unwrap is safe because config validation at startup guarantees this value exists. Consider adding a comment at the unwrap site noting the startup invariant for future readers.",
  "new_findings": []
}
```
