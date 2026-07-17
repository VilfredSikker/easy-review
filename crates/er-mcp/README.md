# er-mcp — Easy Review MCP server

Stdio [Model Context Protocol](https://modelcontextprotocol.io) server for PR triage.
Ask an MCP client things like:

- “Give me the top 5 priority PRs to review”
- “Show me the smallest / low-hanging-fruit PRs”
- “How many production lines changed in PR #42?” (excludes tests, Storybook, generated, docs)
- “Which open PRs are outdated, blocked, failing CI, or waiting on the author?”
- “What’s my review debt?” / “Any stale PRs?” / “Already addressed feedback?”
- “Priority across all my Easy Review projects”
- “Prepare a review kit for PR #42 — I’ll write triage/tour and upload them”

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
| **`prepare_review`** | **Preferred:** write shared `diff-tmp`, return `diff_hash` + prompts (no agent spawn) |
| **`upload_artifacts`** | **Preferred:** validate + write `triage` / `review` / `tour` JSON you produced |
| `run_triage` / `run_review` / `run_tour` / `run_ai_suite` | Optional legacy: spawn local agent CLIs (uses agent slot pool) |
| `list_review_jobs` / `review_job_status` / `cancel_review_job` | Lifecycle for legacy `run_*` jobs |
| `summarize_triage` | Local managed `triage.json` / `review.json` / `tour.json` summary |
| `open_in_easy_review` | GitHub URL + desktop/TUI open instructions |
| `tool_ideas` | Catalog of shipped + future tools |

## Preferred AI path (no slot pool)

The MCP client agent **is** the reviewer. Easy Review only prepares storage and
validates uploads — it does **not** spawn `claude`/`codex`/`agent`, so it never
competes with Desktop/TUI for `agent_slots`.

1. `prepare_review` — fetches the PR diff via `gh`, writes `diff-tmp` into the
   managed PR bucket (`~/.local/share/easy-review/repos/<owner-repo>/prs/pr-<N>/`),
   returns `diff_hash` and the same prepared-diff prompts Desktop uses.
2. You read `diff_tmp_path`, produce the sidecars (embed that exact `diff_hash`).
3. `upload_artifacts` — atomic write + schema/`diff_hash` validation.
4. `summarize_triage` or open the PR in Desktop/TUI — sidecars are shared.

```text
prepare_review     → { "number": 42, "kinds": ["triage", "tour"] }
# …you write the JSON…
upload_artifacts   → { "number": 42, "kind": "tour", "files": { "tour.json": "..." } }
upload_artifacts   → { "number": 42, "kind": "triage", "files": { "triage.json": "..." } }
summarize_triage   → { "number": 42 }
```

**Review uploads** need all four: `review.json`, `order.json`, `checklist.json`, `summary.md`.

## Legacy spawn path (uses slot pool)

`run_triage` / `run_review` / `run_tour` / `run_ai_suite` still spawn agent CLIs
from `~/.config/er/config.toml`. Prefer `prepare_review` + `upload_artifacts`
unless you explicitly want a separate CLI subprocess.

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
prepare_review            → { "number": 42, "kinds": ["tour"] }
upload_artifacts          → { "number": 42, "kind": "tour", "files": { "tour.json": "{...}" } }
summarize_triage          → { "number": 42 }
open_in_easy_review       → { "number": 42 }
```

## Architecture

- Pure ranking / file classification live in `er-engine` (`review_queue`, `git::file_kind`, `git::diff_stats`, `sidecar_summary`).
- Client-owned uploads live in `er-engine::sidecar_upload` (prepare kit + validated write).
- Legacy headless agent runs live in `er-engine::headless_jobs` (optional).
- `er-mcp` is a thin `rmcp` stdio wrapper over those APIs.

## Notes

- `prs_failing_ci` / `prs_blocked` / `prs_already_addressed` fetch per-PR metadata — use `scan_limit` to bound cost.
- `prepare_review` + `upload_artifacts` share storage with Desktop without touching `agent_slots`.
- `run_*` jobs need agent CLIs on PATH and share neither process nor slot pool with Desktop (documented follow-up if you keep using them).
- `open_in_easy_review` returns instructions; there is no `er://` deep-link handler yet.
- Production line counts exclude paths classified as test, Storybook, generated/lock, or docs.
