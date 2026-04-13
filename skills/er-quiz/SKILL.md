# er-quiz

Generate a comprehension quiz for the current diff, writing `.er/quiz.json` for the `er` TUI's Quiz mode (`8` key).

## Trigger

Run as `/er-quiz` or `/er-quiz [scope] [base-branch]`.

## Arguments

Same as `/er-review`:
- `branch` or `1` (default) — full branch diff
- `unstaged` or `2` — uncommitted changes
- `staged` or `3` — staged changes

Optional base branch follows the scope: `/er-quiz branch develop`.

## GitButler awareness

Before Step 1, check if `.er/gb-context.json` exists (Read tool). If it exists and `enabled` is true:

1. Extract `binary`, `selected_branch_id`, and `selected_branch` from the JSON
2. Set `ER_DIR` to `.er/stacks/<selected_branch>/` (create with `mkdir -p`)
3. For the diff capture, use:
   ```
   scripts/er-gb-diff <binary> <selected_branch_id> <ER_DIR>/diff-tmp
   ```
   instead of `git diff <base> ...`. The helper script handles JSON parsing, unified diff reconstruction, shasum, and HEAD output. It matches the allowed `scripts/er-*` pattern.
4. All `.er/` file reads and writes in this skill use `<ER_DIR>/` instead of `.er/`
   (e.g., `<ER_DIR>/quiz.json` instead of `.er/quiz.json`)
5. The persistence cache path becomes `<ER_DIR>/reviews/<branch>/<commit>/`
6. For `diff_hash`, hash the `<ER_DIR>/diff-tmp` file as usual
7. Set `base_branch` in output JSON to the GitButler target branch (from `<binary> config --json`)

If `.er/gb-context.json` does not exist, proceed with the normal git diff flow (backward compatible).

**Permission note:** The GitButler binary path (e.g., `/Applications/GitButler.app/Contents/MacOS/gitbutler-tauri`) is allowed as a first-word command for Bash calls.

## What it does

1. Reads the current diff (same command as `/er-review`)
2. Optionally reads `.er/review.json` if it exists (for additional risk/findings context — not required)
3. Generates quiz questions about significant changes in the diff
4. Writes `.er/quiz.json`

## Review philosophy

See `skills/REVIEW_PHILOSOPHY.md`. Focus questions on significant changes (if `.er/review.json` exists, prioritize P0/P1; otherwise, identify significant changes from the diff directly):

**Question categories:**
- `breaking-changes` — API or contract changes that break callers
- `security` — auth, input validation, secrets, permissions
- `data-integrity` — data loss, corruption, transaction safety
- `logic-paths` — edge cases, error branches, state transitions
- `error-handling` — missing error handling, wrong error propagation
- `test-quality` — shallow tests, missing negative cases

**Never ask about:**
- Naming, formatting, whitespace, style preferences
- File moves or import reordering
- Changes in `info`-only files (no findings, cosmetic only)

## Speed budget

**Target: ≤6 tool calls, ≤45 seconds.**

- TOOL CALL 0: Read `.er/gb-context.json` (GB check — skip if missing; sets `ER_DIR`)
- TOOL CALL 1: Bash — capture diff + hash (GB mode: `<binary> diff`; normal: same as er-review step 1)
- TOOL CALL 2: Read `<ER_DIR>/review.json` (optional — skip if missing, quiz works without it)
- TOOL CALL 3: Read `<ER_DIR>/diff-tmp` (full diff into context)
- IN-CONTEXT: Generate questions — zero tool calls
- TOOL CALL 4: Write `<ER_DIR>/quiz.json`

`<ER_DIR>` is `.er/` normally, or `.er/stacks/<branch>/` in GitButler mode.

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`, and the GitButler binary path when in GB mode.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.

## Question design rules

- **4-8 questions per quiz**. Fewer is better — quality over quantity.
- **Mix MC and freeform**: at least 2 freeform for complex P0/P1 changes. Simple P2-only diffs can be all MC.
- **Difficulty levels** 1-3 (used for filtering in `er`):
  - Level 1 — recall: "What does X do now?"
  - Level 2 — understanding: "Why was X changed to Y?"
  - Level 3 — analysis: "What could go wrong if Z assumption is violated?"
- **MC questions**: 4 options, exactly 1 correct. Distractors should be plausible, not obviously wrong.
- **Freeform questions**: provide `expected_reasoning` with key points a good answer should hit.
- **Pin to files**: always set `related_file` and `related_hunk` where applicable.

## Output schema

### `.er/quiz.json`

```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "questions": [
    {
      "id": "q1",
      "level": 2,
      "category": "security",
      "text": "Why was the JWT algorithm changed from HS256 to RS256?",
      "options": [
        {"label": "A", "text": "RS256 produces shorter tokens", "is_correct": false},
        {"label": "B", "text": "RS256 allows key separation between signing and verification", "is_correct": true},
        {"label": "C", "text": "HS256 is deprecated in the JWT spec", "is_correct": false},
        {"label": "D", "text": "RS256 is faster to verify", "is_correct": false}
      ],
      "freeform": false,
      "explanation": "RS256 uses asymmetric keys: only the auth service needs the private key. With HS256, any service that verifies tokens also holds the signing key — a compromise of any verifier compromises the signer.",
      "related_file": "src/auth.rs",
      "related_hunk": 0,
      "related_lines": [42, 55]
    },
    {
      "id": "q2",
      "level": 3,
      "category": "error-handling",
      "text": "The token refresh path now propagates errors with `?`. What failure modes does this introduce that weren't present before?",
      "freeform": true,
      "expected_reasoning": "Previously errors were swallowed (silent failure). Now they bubble up to the caller. The caller must handle them — if it doesn't, the request may return a 500 instead of a clean 401. Also: if the refresh itself fails transiently (network), the caller may log the user out instead of retrying.",
      "explanation": "Silent failure was arguably worse (user stays logged in with invalid state), but callers need to be updated to handle the new error type.",
      "related_file": "src/auth.rs",
      "related_hunk": 2
    }
  ]
}
```

**Fields:**
- `id` — unique within the quiz (`q1`, `q2`, ...)
- `level` — 1/2/3 (difficulty)
- `category` — one of the six categories above
- `text` — the question text
- `options` — array of 4 options for MC; omit entirely for freeform
- `freeform` — `true` for open-ended, `false` for MC
- `expected_reasoning` — for freeform: key points a good answer should cover (not shown to user until after answering)
- `explanation` — shown after answering (both MC and freeform)
- `related_file` — path to the file this question is about
- `related_hunk` — 0-based hunk index within the file (omit if question spans the whole file)
- `related_lines` — optional `[start, end]` line range on the new side

## Guidelines

- Anchor every question to a specific diff change — no abstract questions about general best practices
- The explanation should teach something, not just restate the correct answer
- For MC, the wrong options should reflect real misunderstandings, not obviously absurd answers
- If `.er/review.json` exists, align question categories with the actual findings (ask about what was flagged)
- If `.er/review.json` does not exist, identify significant changes directly from the diff (structural changes, new logic, error handling, API changes)
- If the diff contains only trivial changes, produce a minimal quiz (2-3 questions) or skip and print "No significant changes to quiz about"
