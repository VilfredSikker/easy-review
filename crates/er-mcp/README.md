# er-mcp — Easy Review MCP server

Stdio [Model Context Protocol](https://modelcontextprotocol.io) server for PR triage.
Ask an MCP client things like:

- “Give me the top 5 priority PRs to review”
- “Show me the smallest / low-hanging-fruit PRs”
- “How many production lines changed in PR #42?” (excludes tests, Storybook, generated, docs)
- “Which open PRs are outdated, blocked, failing CI, or waiting on the author?”
- “What’s my review debt?” / “Any stale PRs?” / “Already addressed feedback?”
- “Priority across all my Easy Review projects”

Uses the authenticated `gh` CLI (same as Easy Review desktop/TUI). Optionally reads
`~/.config/er/projects.json` so you can omit `repo=` when a project is configured.

## Tools

| Tool | Purpose |
|------|---------|
| `list_projects` | Easy Review projects (id, name, remote) |
| `list_prs` | Open PRs with size, review decision, merge state |
| `priority_prs` | Ranked “review next” queue |
| `low_hanging_fruit` | Smallest open PRs (defaults to production-only line enrichment) |
| `cross_repo_queue` | Priority queue across all configured projects |
| `my_review_debt` | Requested of you; you have not approved / requested changes |
| `pr_diff_stats` | Adds/dels split by production / test / storybook / generated / docs |
| `diff_hotspots` | Top production files by churn in a PR |
| `compare_prod_size` | Rank a list of PR numbers by production-only lines |
| `prs_by_status` | Filter: `ready_to_review`, `outdated`, `blocked_conflicts`, `waiting_on_author`, … |
| `prs_stale` | No GitHub activity for N days (default 14) |
| `prs_blocked` | Conflicts, `mergeStateStatus=BLOCKED`, or failing CI |
| `prs_failing_ci` | Failing `gh pr checks` |
| `prs_already_addressed` | All review threads resolved or outdated |
| `run_triage` | Start triage → shared `triage.json` (async job) |
| `run_review` | Start general AI review → shared `review.json` |
| `run_tour` | Generate guided tour → shared `tour.json` |
| `list_review_jobs` / `review_job_status` / `cancel_review_job` | Headless job lifecycle |
| `summarize_triage` | Local managed `triage.json` / `review.json` summary |
| `open_in_easy_review` | GitHub URL + desktop/TUI open instructions |
| `tool_ideas` | Catalog of shipped + future tools |

## Headless reviews (shared storage)

`run_triage` / `run_review` / `run_tour` fetch the PR diff via `gh`, write `diff-tmp` into the same managed PR bucket Desktop uses (`~/.local/share/easy-review/repos/<owner-repo>/prs/pr-<N>/`), and spawn the configured agent from `~/.config/er/config.toml`.

Poll with `review_job_status`, then `summarize_triage` or open the PR in Easy Review Desktop/TUI — sidecars are shared.

Requires agent CLIs on `PATH` (e.g. `claude`, `codex`, `agent`) and `gh auth login`.

```text
run_triage            → { "number": 42 }
review_job_status     → { "id": "hj-1" }
run_tour              → { "number": 42 }
summarize_triage      → { "number": 42 }
```

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

## Example calls

```text
priority_prs              → { "limit": 5, "repo": "acme/widgets" }
low_hanging_fruit         → { "limit": 5 }
my_review_debt            → {}
prs_stale                 → { "days": 14 }
prs_blocked               → { "scan_limit": 20 }
prs_already_addressed     → { "scan_limit": 15 }
cross_repo_queue          → { "limit": 10 }
pr_diff_stats             → { "number": 42 }
diff_hotspots             → { "number": 42, "limit": 10 }
compare_prod_size         → { "numbers": [12, 15, 18] }
summarize_triage          → { "number": 42 }
run_triage                → { "number": 42 }
run_review                → { "number": 42 }
run_tour                  → { "number": 42 }
review_job_status         → { "id": "hj-1" }
open_in_easy_review       → { "number": 42 }
```

## Architecture

- Pure ranking / file classification live in `er-engine` (`review_queue`, `git::file_kind`, `git::diff_stats`, `sidecar_summary`).
- Headless AI runs live in `er-engine::headless_jobs` (shared managed PR buckets + `agent_runtime`).
- `er-mcp` is a thin `rmcp` stdio wrapper over those APIs.

## Notes

- `prs_failing_ci` / `prs_blocked` / `prs_already_addressed` fetch per-PR metadata — use `scan_limit` to bound cost.
- `run_*` jobs need agent CLIs on PATH; they share storage with Desktop but not the Desktop process `agent_slots` pool.
- `open_in_easy_review` returns instructions; there is no `er://` deep-link handler yet.
- Production line counts exclude paths classified as test, Storybook, generated/lock, or docs.
