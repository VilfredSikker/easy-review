# er-quiz-review

Read quiz answers from `.er/quiz-answers.json` and write teaching feedback to `.er/quiz-feedback.json`. Used after a developer completes the quiz in `er`'s Quiz mode (`8` key).

## Trigger

Run as `/er-quiz-review`.

## What it does

1. Reads `.er/quiz.json` (questions + correct answers + expected reasoning)
2. Reads `.er/quiz-answers.json` (developer's answers)
3. Reads `.er/review.json` if it exists (for additional context on findings)
4. Evaluates each answer and writes teaching feedback to `.er/quiz-feedback.json`

## Review philosophy

See `skills/REVIEW_PHILOSOPHY.md`. Feedback should teach, not just grade:

- Explain **why** a wrong MC answer is wrong — what misconception it reflects
- For freeform, evaluate against `expected_reasoning` key points — partial credit is fine
- Don't penalize for not knowing cosmetic/style details — only P0/P1 content counts
- Tone: collegial, direct. "You missed that X also affects Y" not "Incorrect."

## GitButler awareness

Before reading any `.er/` files, check if `.er/gb-context.json` exists (Read tool). If it exists and `enabled` is true, extract `selected_branch` and set `ER_DIR` to `.er/stacks/<selected_branch>/`. All `.er/` file reads and writes use `<ER_DIR>/` instead of `.er/`. If `.er/gb-context.json` does not exist, use `.er/` as normal.

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
   (e.g., `<ER_DIR>/quiz-feedback.json` instead of `.er/quiz-feedback.json`)
5. For `diff_hash`, hash the `<ER_DIR>/diff-tmp` file as usual

If `.er/jj-context.json` does not exist, proceed with the normal git diff flow (backward compatible).

## Speed budget

**Target: ≤5 tool calls, ≤30 seconds.**

- TOOL CALL 1: Read `.er/quiz.json`
- TOOL CALL 2: Read `.er/quiz-answers.json`
- TOOL CALL 3: Read `.er/review.json` (skip if missing)
- IN-CONTEXT: Evaluate answers and generate feedback — zero tool calls
- TOOL CALL 4: Write `.er/quiz-feedback.json`

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`.

## Evaluation rules

**MC questions:**
- Correct → brief reinforcement + one extra insight the explanation might not have covered
- Wrong → explain why the chosen answer is incorrect, why the correct answer is right, and what the misconception likely was

**Freeform questions:**
- Compare against `expected_reasoning` key points
- Partial match (hit some key points) → acknowledge what was right, explain what was missed
- Complete miss → explain the full reasoning from scratch, grounded in the actual diff change
- Strong answer that goes beyond expected_reasoning → acknowledge the insight

**Scoring:**
- MC correct: 1 point
- Freeform: 0 / 0.5 / 1 based on key points covered (none / partial / all)
- Overall score shown in feedback header

## Output schema

### `.er/quiz-feedback.json`

```json
{
  "version": 1,
  "quiz_hash": "<sha256 of quiz.json — for freshness>",
  "score": {
    "correct": 2,
    "partial": 1,
    "total": 3,
    "points": 2.5
  },
  "items": [
    {
      "question_id": "q1",
      "answer_type": "choice",
      "answer_given": "C",
      "correct": true,
      "feedback": "Right. One thing to add: the key separation benefit also means you can rotate the signing key without redeploying every verifier service — they only hold the public key."
    },
    {
      "question_id": "q2",
      "answer_type": "freeform",
      "answer_given": "Errors bubble up to the caller now",
      "correct": false,
      "partial": true,
      "points": 0.5,
      "feedback": "You identified the propagation change correctly. What you missed: the caller at `handle_request()` wasn't updated — it still expects the old return type. This means a refresh failure now returns a 500 instead of triggering a retry or a clean 401. That's the P1 gap flagged in the review."
    }
  ]
}
```

**Fields:**
- `quiz_hash` — SHA-256 of the quiz.json content (detect stale feedback if quiz is regenerated)
- `score.correct` — number of fully correct answers
- `score.partial` — number of partially correct freeform answers
- `score.total` — total number of questions
- `score.points` — numeric score (correct + 0.5*partial)
- `items[].question_id` — matches `id` in quiz.json
- `items[].answer_type` — `"choice"` or `"freeform"`
- `items[].answer_given` — the label (MC) or text (freeform) the developer submitted
- `items[].correct` — boolean (for MC) or false if partial (for freeform)
- `items[].partial` — true if freeform partially covered expected_reasoning
- `items[].points` — 0 / 0.5 / 1
- `items[].feedback` — the teaching text shown in `er`

## Error handling

- If `.er/quiz-answers.json` does not exist: print "No answers found. Complete the quiz in `er` (key `8`) first." and exit.
- If `.er/quiz.json` does not exist: print "No quiz found. Run `/er-quiz` first." and exit.
- If answer IDs in quiz-answers.json don't match question IDs in quiz.json: generate feedback for matched IDs, note which are unmatched.

## Guidelines

- Feedback should be 1-4 sentences per question — enough to teach, not so long it's ignored
- Ground feedback in the actual diff change, not abstract principles
- If the developer got everything right, still provide one additional insight per question (the explanation already exists — add something beyond it)
- Do NOT re-explain what `er` already shows in the explanation field — build on it
