# er-summary

Generate or regenerate the `.er-summary.md` review summary from the current diff and review data.

## Trigger

Run as `/er-summary`.

## What it does

1. Reads the current git diff
2. Reads `.er-review.json` if it exists (for findings context)
3. Reads `.er-feedback.json` if it exists (for human commentary)
4. Writes `.er-summary.md` — a concise, human-readable markdown summary

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
