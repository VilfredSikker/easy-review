---
name: er-review
description: >
  Run Easy Review PR reviews via er-mcp — triage or full review — then upload sidecars into
  shared Desktop/TUI storage. Use when the user says "ER review", "easy review",
  "triage this PR", "upload review artifacts", or gives a PR URL, worktree path, owner/repo,
  or branch to review. For guided tours use the er-guide skill.
metadata:
  author: easy-review
  version: "0.2.0"
---

# Easy Review — review workflow (`er-review`)

You are the reviewer. MCP prepares storage and validates uploads — it does **not** spawn agent CLIs.

See [`../_shared/PREREQUISITES.md`](../_shared/PREREQUISITES.md) and [`../_shared/REF_RESOLUTION.md`](../_shared/REF_RESOLUTION.md).

## Trigger phrases

- "ER review" / "easy review" / "review this PR with ER"
- "triage this PR" / "run triage"
- "upload the review" / "push artifacts to Easy Review"
- User gives a PR URL, worktree path (`/path/to/repo`), `owner/repo`, branch, or PR number

## Default kinds

| User intent | `kinds` on `pr_prepare` |
|-------------|-------------------------|
| "ER review" (unspecified) | `triage` only. Offer full `review` if they want depth. |
| "triage" | `triage` only |
| "tour" / "guide" | use **`er-guide`** skill (`pr_guide`) |
| "full review" / "deep review" | `review` (all four files) |

## Workflow

1. **`pr_resolve`** when `ref` is ambiguous; otherwise pass `ref` on each tool.
2. **`pr_prepare`** with `{ "ref": "…", "kinds": [...] }`.
   - Returns `diff_hash`, `diff_tmp_path`, `artifact_specs`.
3. **Read the diff** at `diff_tmp_path` (managed dir is read-only in sandbox).
4. **Author sidecars** — embed the exact `diff_hash` from `pr_prepare`.
5. **`pr_upload`** once per kind (`triage` or `review` with four files). Tours use **`er-guide`** (`pr_guide`).
6. Optional: **`pr_summarize`**, **`pr_saved`** (`action: pin`) via `er-saved` skill.

## Upload shapes

- Triage: `{ "kind": "triage", "files": { "triage.json": "..." } }`
- Review: `review.json`, `order.json`, `checklist.json`, `summary.md`

## Anti-patterns

- Do not spawn Desktop AI Hub / external CLIs for this flow.
- Do not skip `pr_prepare` or invent a `diff_hash`.
- Do not write files under the managed path — only `pr_upload`.
- Do not auto-pin unless the user asked (use `er-saved`).
