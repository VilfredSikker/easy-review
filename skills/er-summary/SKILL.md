# er-summary

Generate or regenerate the `.er/summary.md` review summary from the current diff and review data.

## Trigger

Run as `/er-summary`.

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
   (e.g., `<ER_DIR>/summary.md` instead of `.er/summary.md`)
5. The persistence cache path becomes `<ER_DIR>/reviews/<branch>/<commit>/`
6. For `diff_hash`, hash the `<ER_DIR>/diff-tmp` file as usual
7. Set `base_branch` in output JSON to the GitButler target branch (from `<binary> config --json`)

If `.er/gb-context.json` does not exist, proceed with the normal git diff flow (backward compatible).

**Permission note:** The GitButler binary path (e.g., `/Applications/GitButler.app/Contents/MacOS/gitbutler-tauri`) is allowed as a first-word command for Bash calls.

## What it does

1. Reads the current git diff
2. Reads `.er/review.json` if it exists (for findings context)
3. Reads `.er/feedback.json` if it exists (for human commentary)
4. Writes `.er/summary.md` — a concise, human-readable markdown summary

## Speed budget

**Target: ≤5 tool calls, ≤30 seconds.**

- TOOL CALL 0: Read `.er/gb-context.json` (GB check — skip if missing; sets `ER_DIR`)
- TOOL CALLS 1-2: Read `<ER_DIR>/review.json` and `<ER_DIR>/feedback.json` (parallel — skip if missing)
- TOOL CALL 3: Bash — diff capture (GB mode: `<binary> diff`; normal: `scripts/er-freshness-check.sh <base>`)
- TOOL CALL 4: Read `<ER_DIR>/diff-tmp` (full diff into context)
- IN-CONTEXT: Generate summary — zero tool calls
- TOOL CALL 5: Write `<ER_DIR>/summary.md`

`<ER_DIR>` is `.er/` normally, or `.er/stacks/<branch>/` in GitButler mode.
Base branch comes from `<ER_DIR>/review.json`. If missing, detect: main then master.

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`, and the GitButler binary path when in GB mode.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.

## Summary structure

```markdown
## Review Summary

<1-2 sentence overall assessment>

### What changed
<Brief description of the changeset's purpose and scope>

### Key concerns
<Bulleted list of the most important findings, if any>

### Risk assessment
<Overall risk: High/Medium/Low with justification>

### Recommendation
<Merge / Merge with changes / Needs work>
```

## Guidelines

- The summary should be useful to someone who hasn't read the diff yet
- Focus on breaking changes, logic changes, and security implications (P0/P1)
- **Do not mention** cosmetic changes (naming, formatting, import reordering, file moves) unless they are the only changes in the diff
- If there are human comments in feedback, incorporate their insights
- Keep it under 30 lines — it's rendered in a TUI
- The diff_hash is NOT stored in the markdown file (it's the only .er/ file without one)
- Write in second person: "You should review..." not "The reviewer should..."
- See `skills/REVIEW_PHILOSOPHY.md` for what counts as P0/P1 vs cosmetic
