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

- MCP server **`easy-review`** connected (`npx -y easy-review-mcp` or `er-mcp` on PATH).
- Authenticated **`gh`** (`gh auth status`).
- Optional: Easy Review projects in `~/.config/er/projects.json` (Desktop writes this).

Managed PR storage (read `diff-tmp` only; never write here directly):

- macOS: `~/Library/Application Support/easy-review/repos/<owner-repo>/prs/pr-<N>/`
- Linux: `~/.local/share/easy-review/repos/<owner-repo>/prs/pr-<N>/`

Always upload sidecars through **`pr_upload`**, not by writing into the managed directory.

Pass the user's target as **`ref`** on any PR-scoped MCP tool, or call **`pr_resolve`** first.

## Accepted `ref` forms

| User types | Example | Resolves via |
|------------|---------|--------------|
| PR URL | `https://github.com/o/r/pull/42` | URL parse |
| Worktree path | `/Users/me/Projects/foo` | git toplevel → open PR for current branch |
| `owner/repo#N` | `acme/widgets#42` | Explicit PR number |
| `owner/repo` | `vilfred/ai-report-builder` | ER project with that remote → open PR in that checkout |
| Branch name | `feature/auth` | `gh pr list --head` in project/cwd repo |
| Bare number | `42` | Active ER project or `repo=` |

## Skill workflow

1. If the user gave a ref (URL, path, slug, branch, number), pass it as `ref` on the tool call.
2. If ambiguous, call **`pr_resolve`** once and use the returned `repo`, `number`, `pr_url`, `bucket_path`.
3. Do not guess a different PR than `pr_resolve` returns.

## Examples

```text
er-review https://github.com/acme/widgets/pull/42
er-review /Users/me/Projects/Discovery/discovery
er-review vilfred/ai-report-builder
er-get-feedback feature/my-branch
```

## Errors

- **No open PR for branch** — ask for a PR URL or `owner/repo#N`.
- **No ER project for owner/repo** — pass a worktree path or configure the project in Desktop.
- **No PR on current branch** — checkout the PR branch or pass an explicit URL.

Creates **`tour.json`** — the pillar-based walkthrough shown in Desktop/TUI **Guide** tab.


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
