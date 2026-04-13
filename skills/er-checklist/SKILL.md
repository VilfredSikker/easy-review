# er-checklist

Generate or update the `.er/checklist.json` review checklist from the current diff and review data.

## Trigger

Run as `/er-checklist`.

## GitButler awareness

Before Step 1, check if `.er/gb-context.json` exists (Read tool). If it exists and `enabled` is true:

1. Extract `binary`, `selected_stack_id`, and `selected_branch` from the JSON
2. Set `ER_DIR` to `.er/stacks/<selected_branch>/` (create with `mkdir -p`)
3. For the diff capture, use:
   ```
   <binary> diff <selected_stack_id> > <ER_DIR>/diff-tmp && shasum -a 256 <ER_DIR>/diff-tmp && git rev-parse --short HEAD
   ```
   instead of `git diff <base> ...`. The binary path from gb-context.json must be added to allowed first-words.
4. All `.er/` file reads and writes in this skill use `<ER_DIR>/` instead of `.er/`
   (e.g., `<ER_DIR>/checklist.json` instead of `.er/checklist.json`)
5. The persistence cache path becomes `<ER_DIR>/reviews/<branch>/<commit>/`
6. For `diff_hash`, hash the `<ER_DIR>/diff-tmp` file as usual
7. Set `base_branch` in output JSON to the GitButler target branch (from `<binary> config --json`)

If `.er/gb-context.json` does not exist, proceed with the normal git diff flow (backward compatible).

**Permission note:** The GitButler binary path (e.g., `/Applications/GitButler.app/Contents/MacOS/gitbutler-tauri`) is allowed as a first-word command for Bash calls.

## What it does

1. Reads the current git diff
2. Reads `.er/review.json` if it exists (to link checklist items to findings)
3. Reads `.er/feedback.json` if it exists (human comments may surface new checklist items)
4. Computes `diff_hash` for staleness detection
5. Writes `.er/checklist.json`

## Speed budget

**Target: ≤5 tool calls, ≤30 seconds.**

- TOOL CALL 0: Read `.er/gb-context.json` (GB check — skip if missing; sets `ER_DIR`)
- TOOL CALLS 1-2: Read `<ER_DIR>/review.json` and `<ER_DIR>/feedback.json` (parallel — skip if missing)
- TOOL CALL 3: Bash — diff capture (GB mode: `<binary> diff`; normal: `scripts/er-freshness-check.sh <base>`)
- TOOL CALL 4: Read `<ER_DIR>/diff-tmp` (full diff into context)
- IN-CONTEXT: Generate checklist — zero tool calls
- TOOL CALL 5: Write `<ER_DIR>/checklist.json`

`<ER_DIR>` is `.er/` normally, or `.er/stacks/<branch>/` in GitButler mode.
Base branch comes from `<ER_DIR>/review.json`. If missing, detect: main then master.

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`, and the GitButler binary path when in GB mode.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.

## Checklist design principles

- Items should be things a human reviewer needs to **manually verify** — not things Claude already checked
- Each item should be specific and actionable: "Confirm the rate limiter config handles burst traffic" not "Check performance"
- **P0/P1 only**: only generate checklist items for high/medium risk concerns. Never generate items about naming, formatting, style, import order, or file moves.
- Link items to findings where relevant via `related_findings`
- Link items to files where relevant via `related_files`
- Categories: `correctness`, `security`, `testing`, `compatibility`, `performance`
- Target 4-8 items for most PRs. More for large or risky changes.
- Pre-check items that are clearly fine (e.g., "No secrets in diff" → checked: true)

**Test quality items** — include when test files are modified:
- "Verify tests for `<function>` assert actual values, not just that the result exists"
- "Confirm there is a negative test for `<changed behavior>` (what should fail or return error)"
- These are P1 concerns and belong on the checklist.

See `skills/REVIEW_PHILOSOPHY.md` for the full list of what to flag and what not to flag.

## Output schema

```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "items": [
    {
      "id": "c-1",
      "text": "Verify the OAuth token refresh handles network timeouts gracefully",
      "category": "correctness",
      "checked": false,
      "related_findings": ["f-1"],
      "related_files": ["src/auth.rs"]
    }
  ]
}
```

## Guidelines

- Don't duplicate findings as checklist items. Findings say what's wrong; checklist items say what to verify.
- Order items by importance (most critical first, P0 before P1)
- The `checked` field defaults to false — the human checks things off in `er`
- If regenerating an existing checklist, preserve `checked` state for items that haven't changed
- No items about naming, formatting, style, or cosmetic changes
