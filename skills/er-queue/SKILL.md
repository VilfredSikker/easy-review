---
name: er-queue
description: >
  Find PRs to review next — priority queue, review debt, blocked, stale, or cross-repo.
  Use when the user asks what to review, review debt, blocked PRs, stale PRs, or
  priority across Easy Review projects.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — review queue (`er-queue`)

See [`../_shared/PREREQUISITES.md`](../_shared/PREREQUISITES.md) and [`../_shared/REF_RESOLUTION.md`](../_shared/REF_RESOLUTION.md).

## Trigger phrases

- "What should I review?" / "priority PRs" / "review queue"
- "My review debt" / "PRs waiting on me"
- "Blocked PRs" / "failing CI" / "stale PRs"
- "Across all my projects" / "cross-repo queue"

## MCP: `prs_query`

| Intent | Call |
|--------|------|
| Priority (default) | `{ "sort": "priority", "limit": 10 }` |
| Review debt | `{ "filter": "review_debt" }` |
| Stale | `{ "filter": "stale", "stale_days": 14 }` |
| Blocked | `{ "filter": "blocked", "scan_limit": 15 }` |
| Failing CI | `{ "filter": "failing_ci" }` |
| Cross-repo | `{ "cross_repo": true, "limit": 10 }` |
| Ready to review | `{ "filter": "ready" }` |

Optional: `production_lines: true` for production-only line enrichment (slower).

Scoped to one repo: pass `ref` as `owner/repo` on `prs_query` (or `repo` + `project_id`).

## Output

Present a compact table: `#`, title, score/reasons, size, status. Offer **`er-review`** on the user's pick — do not auto-review.
