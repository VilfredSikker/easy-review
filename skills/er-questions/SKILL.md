# er-questions

Read personal review questions from `.er/questions.json` and respond with threaded replies. Standalone — does not read or modify `.er/review.json`.

## Trigger

Run as `/er-questions`.

## What it does

1. Reads `.er/questions.json` — personal review questions added via the `er` TUI (press `q` on a line or `Q` on a hunk)
2. Validates freshness: questions must match current diff hash (if stale, warn and abort)
3. For each question needing a response (no replies yet, or last reply author == "user"):
   - Reads the relevant code context from the diff
   - Appends a `Reply` to `question.replies[]`:
     ```json
     { "id": "r-N", "author": "ai", "timestamp": "ISO 8601", "text": "..." }
     ```
4. Writes the updated `.er/questions.json` (with replies added to questions)

Note: Questions are personal/private — they are NOT synced to GitHub. Use `c`/`C` in `er` for GitHub PR comments instead.

## Speed budget

**Target: ~5 tool calls total.**

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `er-freshness-check.sh`.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.
Use `er-freshness-check.sh <base>` for base validation + diff + hash.

## Step-by-step

```
TOOL CALL 1 — Read .er/questions.json
  - If it doesn't exist, check legacy .er-questions.json
  - If neither exists, print "No questions to process" and exit
  - Parse JSON: extract questions array and diff_hash
  - Filter to questions needing response:
    questions where replies is empty, OR last reply has author == "user"

TOOL CALL 2 — Bash (validate freshness + capture diff):
  er-freshness-check.sh <base_branch>
  → Output: "ok", hash line, commit hash
  - Compare hash against questions.diff_hash
  - If questions stale: warn "Questions are stale (diff changed). Skipping." and exit

TOOL CALL 3 — Read .er/diff-tmp (full diff into context)
  - If watched-file questions exist (hunk_index is null), also read those source files directly
  - This is ALL the code context needed. Do NOT read individual source files per comment.

IN-CONTEXT (zero tool calls) — Process all questions from the diff:
  For each question needing a response:

  a. Locate the hunk for question.file / question.hunk_index in the diff already in context
     (for watched-file questions with null hunk_index, use the source file content)
  b. Compose a thoughtful reply referencing actual code
  c. Append to question.replies[]:
     {
       "id": "r-<N>",
       "author": "ai",
       "timestamp": "<ISO 8601>",
       "text": "<thoughtful response referencing actual code from the diff>"
     }

TOOL CALL 4 — Write .er/questions.json (updated with replies on each question)

TOOL CALL 5 — Bash: mkdir -p .er && cp .er/questions.json .er/questions.prev.json

Print summary: "Processed N questions. Added M replies."
```

## Response quality guidelines

- Actually read the code the human is asking about. Don't give generic answers.
- If they disagree with a finding, engage with their reasoning. Sometimes they're right.
- If they ask "why?", explain the actual risk with specifics from their code.
- If they say "I'll fix this" or mark something resolved, acknowledge it and move on.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive about your original review. If you were wrong, say so.

## Example question → reply flow

**Human question** (in .er/questions.json):
```json
{
  "id": "q-1",
  "file": "src/auth.rs",
  "hunk_index": 0,
  "text": "Why is this unwrap safe? Couldn't config be missing?",
  "replies": []
}
```

**After AI processing** (question updated in place):
```json
{
  "id": "q-1",
  "file": "src/auth.rs",
  "hunk_index": 0,
  "text": "Why is this unwrap safe? Couldn't config be missing?",
  "replies": [
    {
      "id": "r-0",
      "author": "ai",
      "timestamp": "2025-01-15T10:30:00Z",
      "text": "Good question. The unwrap is safe because config validation at startup guarantees this value exists. Consider adding a comment at the unwrap site noting the startup invariant."
    }
  ]
}
```

**Follow-up** (user replies via `r` key in TUI, then runs /er-questions again):
```json
{
  "id": "q-1",
  "replies": [
    { "id": "r-0", "author": "ai", "text": "..." },
    { "id": "r-1", "author": "You", "text": "Makes sense, but what about the test environment?" },
    { "id": "r-2", "author": "ai", "text": "In test environments, config is loaded from fixtures which always include this key. The test harness in tests/common.rs sets it up." }
  ]
}
```
