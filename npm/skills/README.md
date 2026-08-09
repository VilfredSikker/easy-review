# @easy-review/skills

Install [Easy Review](https://github.com/VilfredSikker/easy-review) agent skills for Cursor, Claude Code, and Codex.

## Quick start

```bash
cd ~ && npx @easy-review/skills
```

The installer walks you through agents, skills, and install directories (like TechProfessor).

Non-interactive (all agents, all skills, default paths):

```bash
npx -y @easy-review/skills
```

If you run this inside the monorepo `npm/skills` directory, use the home-directory form above.

Install one skill:

```bash
npx -y @easy-review/skills -s er-review
```

List bundled skills:

```bash
npx -y @easy-review/skills --list
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
-g, --global     Install under home agent dirs (default)
-p, --project    Install under project-local agent dirs
-s, --skill      Skill id(s), comma-separated, or *
-a, --agent      Agent ids: cursor, claude-code, codex, all
-y, --yes        Non-interactive: all agents/skills, default paths
-f, --force      Overwrite existing skill folders
```

## Publish

From repo root (after version bump in `package.json`):

```bash
cd npm/skills && npm publish --access public
```

`prepack` syncs `source/` → `skills/` and inlines shared docs.

## Source

Edit skill markdown under [`source/`](source/) (`source/er-review/SKILL.md`, etc.). Shared snippets live in `source/_shared/`. Run `npm run sync` after edits; `prepare`/`prepack` run it automatically.
