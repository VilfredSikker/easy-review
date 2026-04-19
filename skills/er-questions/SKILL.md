# er-questions

Answer personal review questions from `.er/questions.json` by reading the diff directly. Fully standalone — no `.er/review.json` required.

## Trigger

Run as `/er-questions`.

## What it does

1. Reads `.er/questions.json` — personal review questions added via the `er` TUI (press `q` on a line or `Q` on a hunk)
2. Detects the base branch and captures the diff (no external scripts needed)
3. Compares diff hash against `questions.diff_hash` — warns if stale but still proceeds (best-effort)
4. For each unresolved question with no existing AI reply:
   - Locates the relevant hunk in the diff
   - Writes an answer as a new `ReviewQuestion` entry with `in_reply_to` pointing to the question and `author: "Claude"`
   - Marks the original question as `resolved: true`
5. Writes updated `.er/questions.json`
6. Archives to `.er/questions.prev.json`
7. Prints Q&A summary to the Claude terminal

Note: Questions are personal/private — they are NOT synced to GitHub. Use `c`/`C` in `er` for GitHub PR comments instead.

## Speed budget

**Target: ~5 tool calls total.**

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.

## GitButler awareness

Before reading any `.er/` files, check if `.er/gb-context.json` exists (Read tool). If it exists and `enabled` is true, extract `selected_branch` and set `ER_DIR` to `.er/stacks/<selected_branch>/`. All `.er/` file reads and writes use `<ER_DIR>/` instead of `.er/` — questions are stored per-stack when in GitButler mode. If `.er/gb-context.json` does not exist, use `.er/` as normal.

## Jujutsu awareness

Before Step 1, if no GitButler context was found, check if `.er/jj-context.json` exists (Read tool). If it exists and `enabled` is true:

1. Extract `change_id` from the JSON
2. Set `ER_DIR` to `.er/stacks/<change_id>/` (create with `mkdir -p`)
3. For the diff capture, use:
   ```
   scripts/er-jj-diff <change_id> <ER_DIR>/diff-tmp
   ```
   instead of `git diff <base> ...`. The script runs `jj diff -r <change_id> --git` and writes unified diff to the output file. It matches the allowed `scripts/er-*` pattern.
4. All `.er/` file reads and writes in this skill use `<ER_DIR>/` instead of `.er/`
   (e.g., `<ER_DIR>/questions.json` instead of `.er/questions.json`)
5. For `diff_hash`, hash the `<ER_DIR>/diff-tmp` file as usual

If `.er/jj-context.json` does not exist, proceed with the normal git diff flow (backward compatible).

## Step-by-step

```
TOOL CALL 1 — Read .er/questions.json
  - If it doesn't exist or has no unresolved questions (all resolved == true
    and/or no questions without an existing AI reply): print "No questions to process" and exit
  - Parse JSON: extract questions array and diff_hash

TOOL CALL 2 — Bash (detect base branch + capture diff + hash):
  git diff $(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's|refs/remotes/origin/||' || echo main)...HEAD --unified=3 --no-color --no-ext-diff > .er/diff-tmp && shasum -a 256 .er/diff-tmp
  - Compare hash against questions.diff_hash
  - If stale: warn "Questions are stale (diff changed since they were written)." but still proceed

TOOL CALL 3 — Read .er/diff-tmp (full diff into context)
  - This is ALL the code context needed. Do NOT read individual source files.

IN-CONTEXT (zero tool calls) — Answer all unresolved questions:
  For each question where resolved == false and no existing reply (no entry with in_reply_to == question.id):

  a. Locate the hunk for question.file / question.hunk_index in the diff already in context
  b. Write an answer as a NEW entry appended to questions.questions:
     {
       "id": "a-<timestamp>-<seq>",
       "timestamp": "<ISO 8601>",
       "file": "<same as question>",
       "hunk_index": <same as question>,
       "line_start": <same as question>,
       "line_content": <same as question>,
       "text": "<thoughtful answer referencing actual code from the diff>",
       "resolved": false,
       "in_reply_to": "<question.id>",
       "author": "Claude"
     }
  c. Set the original question's resolved field to true

TOOL CALL 4 — Write updated .er/questions.json

TOOL CALL 5 — Bash: cp .er/questions.json .er/questions.prev.json

Print Q&A summary to Claude terminal — for each answered question, show:
  > **Q (src/file.rs:42):** Why is this unwrap safe?
  > **A:** The unwrap is safe because config validation at startup guarantees...
```

## Answer format

- Answers are `ReviewQuestion` entries with `in_reply_to` pointing to the question ID and `author: "Claude"`
- ID prefix `a-` (answer) instead of `q-` (question) for clarity
- Same `file`/`hunk_index`/`line_start`/`line_content` anchoring as the question (so TUI renders them together as reply threads)
- Original question gets `resolved: true`

## Response quality guidelines

- Actually read the code the human is asking about. Don't give generic answers.
- If they disagree with a finding, engage with their reasoning. Sometimes they're right.
- If they ask "why?", explain the actual risk with specifics from their code.
- If they say "I'll fix this" or mark something resolved, acknowledge it and move on.
- Keep responses concise — they render in a TUI with limited width.
- Never be defensive about your original review. If you were wrong, say so.

## Example flow

**Human question** (in .er/questions.json):
```json
{
  "id": "q-1748900000-0",
  "file": "src/auth.rs",
  "hunk_index": 0,
  "line_start": 42,
  "line_content": "    let config = config.unwrap();",
  "text": "Why is this unwrap safe? Couldn't config be missing?",
  "resolved": false
}
```

**AI answer** (appended to questions.questions array):
```json
{
  "id": "a-1748900060-0",
  "timestamp": "2025-06-03T10:01:00Z",
  "file": "src/auth.rs",
  "hunk_index": 0,
  "line_start": 42,
  "line_content": "    let config = config.unwrap();",
  "text": "Good question. The unwrap is safe because config validation at startup guarantees this value exists — see validate_config() in main.rs:15. Consider adding a comment at the unwrap site noting the startup invariant for future readers.",
  "resolved": false,
  "in_reply_to": "q-1748900000-0",
  "author": "Claude"
}
```

**Original question** updated: `"resolved": true`

**Terminal output:**
> **Q (src/auth.rs:42):** Why is this unwrap safe? Couldn't config be missing?
> **A:** Good question. The unwrap is safe because config validation at startup guarantees this value exists — see validate_config() in main.rs:15. Consider adding a comment at the unwrap site noting the startup invariant for future readers.
