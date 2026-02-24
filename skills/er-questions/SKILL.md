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

## Step-by-step

```
1. Read .er-feedback.json
   - If it doesn't exist, print "No feedback to process" and exit
   - Parse JSON, extract comments array

2. Read .er-review.json
   - If it doesn't exist, print "No review to update — run /er-review first" and exit

3. Validate diff_hash
   - Compute current diff hash: git diff <base>...HEAD | sha256sum
   - If feedback.diff_hash != current hash, warn: "Feedback is stale (diff changed). Skipping."
   - If review.diff_hash != current hash, warn: "Review is stale. Run /er-review first."

4. Process each comment:
   For each comment in feedback.comments where resolved == false:

   a. Find context:
      - Read the file at comment.file
      - Look at the hunk at comment.hunk_index
      - Read surrounding code at comment.line_start..comment.line_end

   b. If comment.in_reply_to is set:
      - Find that finding in review.files[comment.file].findings
      - Add a response to finding.responses:
        {
          "id": "r-<n>",
          "in_reply_to": "<comment.id>",
          "timestamp": "<ISO 8601>",
          "text": "<thoughtful response>",
          "new_findings": []
        }

   c. If the comment is a general question (no in_reply_to):
      - Create a new finding if warranted
      - Or add a response to the most relevant existing finding

   d. If the comment says "resolved", "ok", "fixed", etc:
      - Note in response that the concern is addressed
      - Mark the comment's finding with a resolution note

5. Write updated .er-review.json (same diff_hash, updated findings/responses)

6. Copy .er-feedback.json → .er-feedback.prev.json

7. Print summary:
   "Processed N comments. Added M responses, K new findings."
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
