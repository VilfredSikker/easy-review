# er-summary

Generate or regenerate the `.er-summary.md` review summary from the current diff and review data.

## Trigger

Run as `/er-summary`.

## What it does

1. Reads the current git diff
2. Reads `.er-review.json` if it exists (for findings context)
3. Reads `.er-feedback.json` if it exists (for human commentary)
4. Writes `.er-summary.md` — a concise, human-readable markdown summary

## Speed budget

**Target: ≤5 tool calls, ≤30 seconds.**

- TOOL CALLS 1-2: Read .er-review.json and .er-feedback.json (parallel — skip if missing)
- TOOL CALL 3: Bash — `scripts/er-freshness-check.sh <base>` (captures diff + hash)
- TOOL CALL 4: Read .er-diff-tmp (full diff into context)
- IN-CONTEXT: Generate summary — zero tool calls
- TOOL CALL 5: Write .er-summary.md

Base branch comes from .er-review.json. If missing, detect: main then master.

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`.
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
- Focus on *what matters*, not on listing every file that changed
- If there are human comments in feedback, incorporate their insights
- Keep it under 30 lines — it's rendered in a TUI
- The diff_hash is NOT stored in the markdown file (it's the only .er-* file without one)
- Write in second person: "You should review..." not "The reviewer should..."
