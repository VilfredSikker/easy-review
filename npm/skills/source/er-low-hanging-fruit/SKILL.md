---
name: er-low-hanging-fruit
description: >
  Find the smallest / quickest PRs to review using production-only line counts.
  Use when the user asks for low-hanging fruit, smallest PRs, quick wins, or wants
  to compare PR sizes. Optional ref scopes to one repo.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — low-hanging fruit (`er-low-hanging-fruit`)

See [`../_shared/PREREQUISITES.md`](../_shared/PREREQUISITES.md) and [`../_shared/REF_RESOLUTION.md`](../_shared/REF_RESOLUTION.md).

## Trigger phrases

- "Low-hanging fruit" / "smallest PRs" / "quick wins"
- "Compare PR sizes" / "which PR is smallest?"

## MCP calls

**Ranked smallest (default):**

```json
{
  "sort": "smallest",
  "production_lines": true,
  "limit": 5,
  "repo": "owner/name"
}
```

Use `ref: "owner/repo"` or `projects_list` when repo omitted.

**Compare specific PR numbers:**

```json
{ "ref": "owner/repo", "numbers": [12, 15, 18] }
```

via **`pr_stats`** with `numbers` array.

**Single PR depth:**

```json
{ "ref": "https://github.com/o/r/pull/42", "include_hotspots": true }
```

## Output

Table: `#`, title, production lines, total lines, review state. Offer **`er-review`** on pick — do not auto-review.
