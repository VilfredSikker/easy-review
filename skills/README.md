# er Skills for Claude Code

These skills power the AI feedback loop in `er`. They generate `.er-*` sidecar files that `er` reads and renders in the AI overlay.

## Setup (local, pre-plugin)

Copy the skill folders into your Claude Code project commands:

```bash
# From the easy-review repo root:
cp -r skills/er-review ~/.claude/commands/er-review
cp -r skills/er-questions ~/.claude/commands/er-questions
cp -r skills/er-risk-sort ~/.claude/commands/er-risk-sort
cp -r skills/er-summary ~/.claude/commands/er-summary
cp -r skills/er-checklist ~/.claude/commands/er-checklist
```

Or symlink them for auto-updates:
```bash
ln -s $(pwd)/skills/er-review ~/.claude/commands/er-review
ln -s $(pwd)/skills/er-questions ~/.claude/commands/er-questions
ln -s $(pwd)/skills/er-risk-sort ~/.claude/commands/er-risk-sort
ln -s $(pwd)/skills/er-summary ~/.claude/commands/er-summary
ln -s $(pwd)/skills/er-checklist ~/.claude/commands/er-checklist
```

## Workflow

```
Terminal 1: er (TUI)          Terminal 2: Claude Code
─────────────────────         ──────────────────────

                              /er-review
                              → writes .er-*.json files

er auto-detects new files
press v → AI Overlay mode
review findings inline
press c → add comment on hunk

                              /er-questions
                              → reads .er-feedback.json
                              → adds responses to findings
                              → archives old feedback

er auto-refreshes
see AI responses inline
continue reviewing...
```

## Skills

| Skill | What it does |
|-------|-------------|
| `er-review` | Full review: risk levels, findings, order, checklist, summary |
| `er-questions` | Process human feedback, respond to comments, add new findings |
| `er-risk-sort` | Re-sort file review order by risk and logical grouping |
| `er-summary` | Regenerate the markdown summary |
| `er-checklist` | Regenerate the review checklist |

## Testing without skills

Use the test fixture generator to create sample data:

```bash
cd your-repo
bash /path/to/easy-review/scripts/generate-test-fixtures.sh
```

This creates .er-* files with matching diff_hash so you can test the overlay rendering.
