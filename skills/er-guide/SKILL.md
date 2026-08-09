---
name: er-guide
description: >
  Create an Easy Review guided tour (tour.json) for a PR via er-mcp. Use when the user
  asks for a guide, guided tour, walkthrough, or tour for a PR. Accepts PR URL, worktree
  path, owner/repo, branch, or number.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review — guided tour (`er-guide`)

Creates **`tour.json`** — the pillar-based walkthrough shown in Desktop/TUI **Guide** tab.

See [`../_shared/PREREQUISITES.md`](../_shared/PREREQUISITES.md) and [`../_shared/REF_RESOLUTION.md`](../_shared/REF_RESOLUTION.md).

## Trigger phrases

- "Create a guide" / "guided tour" / "walk me through this PR"
- "Generate tour" / "ER guide" / "tour.json"
- User gives PR URL, worktree path, `owner/repo`, branch, or number

## Workflow

1. **`pr_resolve`** when `ref` is ambiguous; otherwise pass `ref` on each call.
2. **`pr_guide`** `{ "ref": "…", "action": "prepare" }` (default action).
   - Writes `diff-tmp`, returns `diff_hash`, `diff_tmp_path`, `artifact_specs` for **tour only**.
3. **Read the diff** at `diff_tmp_path`.
4. **Author `tour.json`** — embed exact `diff_hash`; 3–7 pillars; every changed file once.
5. **`pr_guide`** `{ "ref": "…", "action": "upload", "files": { "tour.json": "..." } }`.
6. Optional: **`pr_summarize`**, **`pr_saved`** (`action: pin`).

## Tour rules (from artifact spec)

- One `tour.json` only — no separate triage/review files.
- Foundation / core pillars first; group related files under `related[]` when helpful.
- Titles and blurbs should explain *why* each area matters, not just what changed.

## Anti-patterns

- Do not use `pr_prepare` + `pr_upload` for tour-only work — use **`pr_guide`** (smaller payload).
- Do not skip `prepare` or invent `diff_hash`.
- Do not spawn Desktop AI Hub tour agents — you author the JSON.
