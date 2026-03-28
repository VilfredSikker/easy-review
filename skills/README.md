# er Skills for Claude Code

These skills power the AI feedback loop in `er`. They generate `.er/` sidecar files that `er` reads and renders in the AI overlay.

## Review Philosophy

All skills follow a shared severity model and "what not to flag" list. See `skills/REVIEW_PHILOSOPHY.md` before modifying any skill.

**TL;DR:** P0 (high) = must fix, P1 (medium) = should fix, P2 (low) = nice to fix. Never flag naming, formatting, style, or file moves.

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
cp -r skills/er-quiz ~/.claude/commands/er-quiz
cp -r skills/er-quiz-review ~/.claude/commands/er-quiz-review
```

Or symlink them for auto-updates:
```bash
ln -s $(pwd)/skills/er-review ~/.claude/commands/er-review
ln -s $(pwd)/skills/er-questions ~/.claude/commands/er-questions
ln -s $(pwd)/skills/er-risk-sort ~/.claude/commands/er-risk-sort
ln -s $(pwd)/skills/er-summary ~/.claude/commands/er-summary
ln -s $(pwd)/skills/er-checklist ~/.claude/commands/er-checklist
ln -s $(pwd)/skills/er-publish ~/.claude/commands/er-publish
ln -s $(pwd)/skills/er-quiz ~/.claude/commands/er-quiz
ln -s $(pwd)/skills/er-quiz-review ~/.claude/commands/er-quiz-review
```

## Workflow

| Step | Where | What happens |
|------|-------|--------------|
| 1 | Claude Code | `/er-review` — writes `.er/*.json` files |
| 2 | er (TUI) | Auto-detects new files, press `v` for AI Overlay mode |
| 3 | er (TUI) | Review findings inline, press `c` to comment on a hunk |
| 4 | Claude Code | `/er-questions` — reads feedback, responds, archives |
| 5 | er (TUI) | Auto-refreshes, see AI responses inline, continue reviewing |
| 6 | Claude Code | `/er-publish` — validates freshness, posts to GitHub PR |

**Quiz workflow (optional):**

| Step | Where | What happens |
|------|-------|--------------|
| 1 | Claude Code | `/er-quiz` — generates `.er/quiz.json` from the diff |
| 2 | er (TUI) | Press `8` for Quiz mode, answer questions |
| 3 | Claude Code | `/er-quiz-review` — evaluates answers, writes `.er/quiz-feedback.json` |
| 4 | er (TUI) | Auto-refreshes, feedback shown inline |

## Skills

| Skill | What it does |
|-------|-------------|
| `er-review` | Full review: risk levels, findings, order, checklist, summary |
| `er-questions` | Process human feedback, respond to comments, add new findings |
| `er-risk-sort` | Re-sort file review order by P0→P1→P2, cosmetic files last |
| `er-summary` | Regenerate the markdown summary (P0/P1 focus, no cosmetic noise) |
| `er-checklist` | Regenerate the review checklist (P0/P1 only, includes test-quality items) |
| `er-publish` | Publish review findings to GitHub PR as inline comments |
| `er-quiz` | Generate comprehension quiz questions about P0/P1 changes |
| `er-quiz-review` | Evaluate quiz answers and write teaching feedback |

## Testing without skills

Use the test fixture generator to create sample data:

```bash
cd your-repo
bash /path/to/easy-review/scripts/generate-test-fixtures.sh
```

This creates .er/ files with matching diff_hash so you can test the overlay rendering.
