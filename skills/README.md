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
cp -r skills/er-publish ~/.claude/commands/er-publish
```

Or symlink them for auto-updates:
```bash
ln -s $(pwd)/skills/er-review ~/.claude/commands/er-review
ln -s $(pwd)/skills/er-questions ~/.claude/commands/er-questions
ln -s $(pwd)/skills/er-risk-sort ~/.claude/commands/er-risk-sort
ln -s $(pwd)/skills/er-summary ~/.claude/commands/er-summary
ln -s $(pwd)/skills/er-checklist ~/.claude/commands/er-checklist
ln -s $(pwd)/skills/er-publish ~/.claude/commands/er-publish
```

## Workflow

| Step | Where | What happens |
|------|-------|--------------|
| 1 | Claude Code | `/er-review` — writes `.er-*.json` files |
| 2 | er (TUI) | Auto-detects new files, press `v` for AI Overlay mode |
| 3 | er (TUI) | Review findings inline, press `c` to comment on a hunk |
| 4 | Claude Code | `/er-questions` — reads feedback, responds, archives |
| 5 | er (TUI) | Auto-refreshes, see AI responses inline, continue reviewing |
| 6 | Claude Code | `/er-publish` — validates freshness, posts to GitHub PR |

## Skills

| Skill | What it does |
|-------|-------------|
| `er-review` | Full review: risk levels, findings, order, checklist, summary |
| `er-questions` | Process human feedback, respond to comments, add new findings |
| `er-risk-sort` | Re-sort file review order by risk and logical grouping |
| `er-summary` | Regenerate the markdown summary |
| `er-checklist` | Regenerate the review checklist |
| `er-publish` | Publish review findings to GitHub PR as inline comments |

## Testing without skills

Use the test fixture generator to create sample data:

```bash
cd your-repo
bash /path/to/easy-review/scripts/generate-test-fixtures.sh
```

This creates .er-* files with matching diff_hash so you can test the overlay rendering.
