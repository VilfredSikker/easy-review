---
name: er-saved
description: >
  Pin, unpin, or list saved PRs and uploaded Easy Review artifacts in Desktop Saved
  and managed storage. Use when the user wants to pin a reviewed PR, list saved work,
  or find PRs with triage/review/tour sidecars.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — saved PRs (`er-saved`)

See [`../_shared/REF_RESOLUTION.md`](../_shared/REF_RESOLUTION.md).

## Trigger phrases

- "Pin this PR" / "save to Desktop"
- "List saved PRs" / "what have I reviewed?"
- "Show uploaded artifacts"

## MCP: `pr_saved`

| Action | Call |
|--------|------|
| Pin | `{ "action": "pin", "ref": "…" }` |
| Unpin | `{ "action": "unpin", "ref": "…" }` |
| Saved only | `{ "action": "list", "source": "pinned" }` |
| Artifacts scan | `{ "action": "list", "source": "artifacts" }` |
| Both | `{ "action": "list", "source": "all" }` |

Optional `kinds: ["triage", "tour"]` on list to filter artifacts.

Only pin when the user asks or after a successful **`er-review`** they asked to save.
