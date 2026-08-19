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

You are the reviewer. MCP prepares storage and validates uploads — it does **not** spawn agent CLIs.


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
