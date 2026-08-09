# @easy-review/skills

Install [Easy Review](https://github.com/VilfredSikker/easy-review) agent skills for Cursor, Claude Code, Codex, and other agents that support the [skills](https://www.npmjs.com/package/skills) installer.

## Quick start

```bash
bunx @easy-review/skills
# or
npx @easy-review/skills
```

Install one skill:

```bash
bunx @easy-review/skills -s er-review
```

List bundled skills:

```bash
bunx @easy-review/skills --list
```

## Bundled skills

| Skill | Purpose |
|-------|---------|
| `er-review` | Prepare → author → upload triage/review sidecars |
| `er-guide` | Create guided tour (`tour.json`) |
| `er-queue` | What to review next (priority, debt, blocked, stale) |
| `er-low-hanging-fruit` | Smallest / quick-win PRs |
| `er-get-feedback` | Read questions, notes, findings |
| `er-respond` | Reply on PR threads |
| `er-saved` | Pin / list saved PRs and artifacts |

Pair with the MCP server:

```bash
bunx easy-review-mcp   # stdio MCP — see docs/guide/mcp.html
```

## Options

```
-g, --global     Install to user-level agent dirs (default)
-p, --project    Install project-local
-s, --skill      Skill name or * (default: all)
-a, --agent      Agent id (cursor, claude-code, codex, …)
```

## Publish

From repo root (after version bump in `package.json`):

```bash
cd npm/skills && npm publish --access public
```

`prepack` syncs skills from `../../skills/` and inlines shared docs.

## Source

Canonical skill sources: [`skills/`](../../skills/) at the repo root.
