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
| `prepare_review` | Write shared `diff-tmp`, return `diff_hash` + prompts |
| `get_artifact_specs` | JSON Schema + examples + prompts for triage/review/tour (no PR needed) |
| `upload_artifacts` | Validate + write `triage` / `review` / `tour` JSON you produced |
| `summarize_triage` | Local managed `triage.json` / `review.json` / `tour.json` summary |
| `open_in_easy_review` | GitHub URL + desktop/TUI open instructions |
| `tool_ideas` | Catalog of shipped + future tools |

## AI sidecars (client-owned)

The MCP client agent **is** the reviewer. Easy Review prepares storage and
validates uploads — it does not spawn agent CLIs.

1. `get_artifact_specs` — JSON Schema, examples, and Desktop prepared-diff prompts
   (optional but recommended before authoring).
2. `prepare_review` — fetches the PR diff via `gh`, writes `diff-tmp` into the
   managed PR bucket (`~/.local/share/easy-review/repos/<owner-repo>/prs/pr-<N>/`),
   returns `diff_hash` and prompts with the real output path.
3. You read `diff_tmp_path`, produce the sidecars (embed that exact `diff_hash`).
4. `upload_artifacts` — atomic write + schema/`diff_hash` validation.
5. `summarize_triage` or open the PR in Desktop/TUI — sidecars are shared.

```text
get_artifact_specs → { "kinds": ["tour"] }
prepare_review     → { "number": 42, "kinds": ["triage", "tour"] }
# …you write the JSON per schema…
upload_artifacts   → { "number": 42, "kind": "tour", "files": { "tour.json": "..." } }
summarize_triage   → { "number": 42 }
```

**Review uploads** need all four: `review.json`, `order.json`, `checklist.json`, `summary.md`.

`upload_artifacts` validates serde deserialization + matching `diff_hash` **before** writing.
It does **not** enforce the full JSON Schema from `get_artifact_specs` — use the schemas as the
authoring contract.

## Build / run

```bash
# npm launcher (downloads the release binary on first run)
npx -y easy-review-mcp

# From source
cargo build -p er-mcp --release
# binary: target/release/er-mcp

cargo install --path crates/er-mcp
# → ~/.cargo/bin/er-mcp
```

Requires `gh auth login`.

## Client setup (Claude / Cursor / Codex)

Full guide with **`mcp.json`**, **`claude mcp add`**, and **`codex mcp add`**:
[docs/guide/mcp.html](../../docs/guide/mcp.html)
([published](https://vilfredsikker.github.io/easy-review/guide/mcp.html)).

npm launcher (package: [`npm/er-mcp`](../../npm/er-mcp)):

```bash
# Claude Code
claude mcp add --scope user easy-review -- npx -y easy-review-mcp

# Codex
codex mcp add easy-review -- npx -y easy-review-mcp
```

Cursor — `~/.cursor/mcp.json` or `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "easy-review": {
      "command": "npx",
      "args": ["-y", "easy-review-mcp"]
    }
  }
}
```

Or point `command` at a source-built `/absolute/path/to/er-mcp`.

### Agent skill (“ER review”)

Install the companion skill so agents know to call `prepare_review` → author →
`upload_artifacts` when you say **“ER review”** (same as `npx skills add github/gh-stack`):

```bash
npx skills add VilfredSikker/easy-review -s er-review -g   # global; prompts for agents
npx skills add VilfredSikker/easy-review -s er-review      # project
npx skills add VilfredSikker/easy-review -s er-review -g -a cursor -y
```

Skill source: [`skills/er-review/SKILL.md`](../../skills/er-review/SKILL.md).

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
get_artifact_specs        → { "kinds": ["tour", "triage"] }
upload_artifacts          → { "number": 42, "kind": "tour", "files": { "tour.json": "{...}" } }
summarize_triage          → { "number": 42 }
open_in_easy_review       → { "number": 42 }
```

## Architecture

- Pure ranking / file classification live in `er-engine` (`review_queue`, `git::file_kind`, `git::diff_stats`, `sidecar_summary`).
- Client-owned uploads live in `er-engine::sidecar_upload` (prepare kit + validated write).
- JSON Schema + prompt contracts live in `er-engine::sidecar_specs` (`get_artifact_specs`).
- `er-mcp` is a thin `rmcp` stdio wrapper over those APIs.

## Notes

- `prs_failing_ci` / `prs_blocked` / `prs_already_addressed` fetch per-PR metadata — use `scan_limit` (capped at 20) to bound cost; enrichments run in parallel.
- `compare_prod_size` caps at 12 PRs and fetches diffs in parallel.
- `prepare_review` + `upload_artifacts` share storage with Desktop without touching `agent_slots`.
- `open_in_easy_review` returns instructions; there is no `er://` deep-link handler yet.
- Production line counts exclude paths classified as test, Storybook, generated/lock, or docs.
