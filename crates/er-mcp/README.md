# er-mcp — Easy Review MCP server

Stdio [Model Context Protocol](https://modelcontextprotocol.io) server for PR triage.
Ask an MCP client things like:

- “Give me the top 5 priority PRs to review”
- “Show me the smallest / low-hanging-fruit PRs”
- “How many production lines changed in PR #42?” (excludes tests, Storybook, generated, docs)
- “Which open PRs are outdated or blocked?”

Uses the authenticated `gh` CLI (same as Easy Review desktop/TUI). Optionally reads
`~/.config/er/projects.json` so you can omit `repo=` when a project is configured.

## Tools

| Tool | Purpose |
|------|---------|
| `list_projects` | Easy Review projects (id, name, remote) |
| `list_prs` | Open PRs with size, review decision, merge state |
| `priority_prs` | Ranked “review next” queue (`limit`, optional `production_lines`) |
| `low_hanging_fruit` | Smallest open PRs (defaults to production-only line enrichment) |
| `pr_diff_stats` | Per-PR adds/dels split by `production` / `test` / `storybook` / `generated` / `docs` |
| `prs_by_status` | Filter: `ready_to_review`, `outdated`, `blocked_conflicts`, `waiting_on_author`, `approved`, `merge_ready`, `draft` |
| `tool_ideas` | Backlog of additional MCP tools worth adding |

## Build / run

```bash
cargo build -p er-mcp --release
# binary: target/release/er-mcp
```

Requires `gh auth login`.

### Cursor MCP config

```json
{
  "mcpServers": {
    "easy-review": {
      "command": "/absolute/path/to/er-mcp"
    }
  }
}
```

Or during development:

```json
{
  "mcpServers": {
    "easy-review": {
      "command": "cargo",
      "args": ["run", "-q", "-p", "er-mcp"],
      "cwd": "/absolute/path/to/easy-review"
    }
  }
}
```

## Example calls

```text
priority_prs            → { "limit": 5, "repo": "acme/widgets" }
low_hanging_fruit       → { "limit": 5, "production_lines": true }
pr_diff_stats           → { "number": 42, "include_files": false }
prs_by_status           → { "status": "outdated" }
```

## Architecture

- Pure ranking / file classification live in `er-engine` (`review_queue`, `git::file_kind`, `git::diff_stats`) so they stay unit-testable without MCP.
- `er-mcp` is a thin `rmcp` stdio wrapper that shells out via `er-engine::github`.

## Future tools (see also `tool_ideas`)

- **Already fixed / addressed** — review threads resolved or outdated after new commits
- **Failing CI** — required checks red
- **My review debt** — requested of me, no review yet
- **Stale PRs** — no activity for N days
- **Cross-repo queue** — rank across all configured projects
- **Open in Easy Review** — deep-link into the desktop app
- **Triage sidecar summary** — read managed `triage.json` / `review.json` when present
