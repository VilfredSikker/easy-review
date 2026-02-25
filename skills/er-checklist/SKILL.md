# er-checklist

Generate or update the `.er-checklist.json` review checklist from the current diff and review data.

## Trigger

Run as `/er-checklist`.

## What it does

1. Reads the current git diff
2. Reads `.er-review.json` if it exists (to link checklist items to findings)
3. Reads `.er-feedback.json` if it exists (human comments may surface new checklist items)
4. Computes `diff_hash` for staleness detection
5. Writes `.er-checklist.json`

## Speed budget

**Target: ≤5 tool calls, ≤30 seconds.**

- TOOL CALLS 1-2: Read .er-review.json and .er-feedback.json (parallel — skip if missing)
- TOOL CALL 3: Bash — `scripts/er-freshness-check.sh <base>` (captures diff + hash)
- TOOL CALL 4: Read .er-diff-tmp (full diff into context)
- IN-CONTEXT: Generate checklist — zero tool calls
- TOOL CALL 5: Write .er-checklist.json

Base branch comes from .er-review.json. If missing, detect: main then master.

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.

## Checklist design principles

- Items should be things a human reviewer needs to **manually verify** — not things Claude already checked
- Each item should be specific and actionable: "Confirm the rate limiter config handles burst traffic" not "Check performance"
- Link items to findings where relevant via `related_findings`
- Link items to files where relevant via `related_files`
- Categories: `correctness`, `security`, `testing`, `compatibility`, `performance`, `documentation`
- Target 4-8 items for most PRs. More for large or risky changes.
- Pre-check items that are clearly fine (e.g., "No secrets in diff" → checked: true)

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
- Order items by importance (most critical first)
- The `checked` field defaults to false — the human checks things off in `er`
- If regenerating an existing checklist, preserve `checked` state for items that haven't changed
