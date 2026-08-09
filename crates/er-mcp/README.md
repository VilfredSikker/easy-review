# er-mcp — Easy Review MCP server

Stdio [Model Context Protocol](https://modelcontextprotocol.io) server (MCP **2026-07-28**).
Thin REST API over `er-engine` — use **skills** for workflows.

## MCP tools (11)

| Tool | Purpose |
|------|---------|
| `projects_list` | Easy Review projects (`~/.config/er/projects.json`) |
| `pr_resolve` | `ref` → owner, repo, number, `pr_url`, `bucket_path` |
| `prs_query` | List/rank/filter open PRs (`sort`, `filter`, `cross_repo`) |
| `pr_stats` | Production vs test/docs diff stats; batch + hotspots |
| `pr_guide` | Prepare + upload guided tour (`tour.json`) |
| `pr_prepare` | Fetch diff, write `diff-tmp`, return `diff_hash` + specs |
| `pr_upload` | Validate + write triage/review/tour sidecars |
| `pr_summarize` | Read triage/review/tour summary from managed storage |
| `pr_feedback_get` | Questions, notes, AI findings |
| `pr_feedback_reply` | Reply (`type`: question \| note \| finding) |
| `pr_saved` | Pin / unpin / list saved PRs and artifacts |

## Universal `ref` targeting

Every PR-scoped tool accepts **`ref`** (preferred) or `pr_url` / `repo` + `number`:

| `ref` | Example |
|-------|---------|
| PR URL | `https://github.com/acme/widgets/pull/42` |
| Worktree path | `/Users/me/Projects/foo` |
| `owner/repo#N` | `acme/widgets#42` |
| `owner/repo` | `vilfred/ai-report-builder` (ER project → open PR in checkout) |
| Branch | `feature/auth` |
| Number | `42` (with active ER project) |

Call `pr_resolve` first when ambiguous.

## Skills (workflows)

Install with **`bunx @easy-review/skills`** (or `npx @easy-review/skills`):

| Skill | Intent |
|-------|--------|
| `er-review` | Prepare → author → upload sidecars |
| `er-guide` | Guided tour only (`tour.json`) |
| `er-queue` | What to review next (priority, debt, blocked, stale) |
| `er-low-hanging-fruit` | Smallest / quick-win PRs |
| `er-get-feedback` | Read questions, notes, findings |
| `er-respond` | Reply on PR threads |
| `er-saved` | Pin / list saved work |

One skill: `bunx @easy-review/skills -s er-review`. Source: [`skills/`](../../skills/).

Guided tour:

```text
pr_guide     → { "ref": "…", "action": "prepare" }
pr_guide     → { "ref": "…", "action": "upload", "files": { "tour.json": "..." } }
```

Full review flow:

```text
pr_prepare   → { "ref": "…", "kinds": ["triage"] }
# …author JSON per artifact_specs…
pr_upload    → { "ref": "…", "kind": "triage", "files": { "triage.json": "..." } }
pr_summarize → { "ref": "…" }
```

Guided tours use **`pr_guide`** (not `pr_prepare` / `pr_upload`). See `er-guide` skill.

Review uploads need all four: `review.json`, `order.json`, `checklist.json`, `summary.md`.

## `prs_query` reference

```json
{
  "sort": "priority | smallest | updated",
  "filter": "review_debt | stale | blocked | failing_ci | ready | addressed | …",
  "cross_repo": false,
  "production_lines": false,
  "limit": 10,
  "scan_limit": 15,
  "stale_days": 14
}
```

## Build / run

```bash
npx -y easy-review-mcp
cargo build -p er-mcp --release
cargo install --path crates/er-mcp
```

Requires `gh auth login`.

Setup: [docs/guide/mcp.html](../../docs/guide/mcp.html).

## Architecture

- Ranking, diff stats, sidecar I/O live in `er-engine`.
- `pr_resolve` lives in `er-engine::pr_resolve`.
- `er-mcp` is a thin `rmcp` stdio wrapper.
