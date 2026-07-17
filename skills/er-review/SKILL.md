---
name: er-review
description: >
  Run Easy Review PR reviews via the er-mcp tools — triage, full review, or guided
  tour — then upload sidecars into shared Desktop/TUI storage. Use when the user says
  "ER review", "easy review", "triage this PR", "upload review artifacts", or asks to
  prepare/review a GitHub PR with Easy Review MCP.
metadata:
  author: easy-review
  version: "0.1.0"
---

# Easy Review (er-mcp)

You are the reviewer. Easy Review MCP prepares storage and validates uploads — it does
**not** spawn agent CLIs. Sidecars land in the same managed path Desktop/TUI read:

`~/.local/share/easy-review/repos/<owner-repo>/prs/pr-<N>/`

## Prerequisites

- MCP server `easy-review` connected (`er-mcp` binary).
- Authenticated `gh` (`gh auth status`).
- Prefer configured Easy Review projects (`list_projects`) so `repo=` can be omitted.

## Trigger phrases

Treat these as requests to run this skill end-to-end (not just list tools):

- "ER review" / "easy review" / "review this PR with ER"
- "triage this PR" / "run triage"
- "guided tour" / "generate tour" (when they mean Easy Review tour.json)
- "upload the review" / "push artifacts to Easy Review"
- "pin this PR" / "show pinned reviews" / "what have I reviewed"

## Resolve the PR

1. If the user gave a PR number and/or `owner/repo`, use those.
2. Else call `list_projects` / infer from the current git remote when obvious.
3. If still ambiguous, ask once for `number` (and `repo` if needed). Do not guess a wrong PR.

## Default kinds

| User intent | `kinds` / uploads |
|-------------|-------------------|
| "ER review" (unspecified) | `triage` + `tour` (fast, high value). Offer full `review` if they want depth. |
| "triage" | `triage` only |
| "tour" / "guide" | `tour` only |
| "full review" / "deep review" | `review` (all four files) |

## Workflow (always)

1. **`get_artifact_specs`** with the kinds you will author (schemas, examples, prompts).
2. **`prepare_review`** with `{ "number": N, "kinds": [...], "repo": "owner/name" }` when needed.
   - Writes shared `diff-tmp` under the managed PR bucket.
   - Returns `diff_hash`, `diff_tmp_path`, and `artifact_specs` (use prompts on specs; ignore duplicate kit artifact prompts).
3. **Read the prepared diff** at `diff_tmp_path` (and follow the prepared-diff prompts).
4. **Author sidecar JSON/Markdown** yourself:
   - Embed the **exact** `diff_hash` from `prepare_review` in every JSON that requires it.
   - Follow schemas/examples from `get_artifact_specs` / `artifact_specs`.
5. **`upload_artifacts`** once per kind:
   - Triage: `{ "kind": "triage", "files": { "triage.json": "..." } }`
   - Tour: `{ "kind": "tour", "files": { "tour.json": "..." } }`
   - Review: all four — `review.json`, `order.json`, `checklist.json`, `summary.md`
6. **`pin_pr`** (optional but recommended) so the PR lands in Desktop Saved and is easy to find later via `list_pinned_prs`.
7. **`summarize_triage`** (optional) and/or **`open_in_easy_review`** so the user can open Desktop/TUI.

Do **not** write files under the managed path yourself — always go through `upload_artifacts`.
Do **not** auto-pin — only call `pin_pr` when the user wants it bookmarked, or after a successful review when they asked to save/pin.

## Finding reviewed work

- **`list_pinned_prs`** — Desktop Saved PRs (explicit pins), with sidecar presence.
- **`list_artifacts`** — scan managed storage for any uploaded triage/review/tour (pinned or not).
- **`unpin_pr`** — remove from Desktop Saved.

## Validation rules

- `upload_artifacts` checks serde shape + matching `diff_hash` **before** writing.
- It does **not** enforce full JSON Schema — treat `get_artifact_specs` as the authoring contract.
- If upload fails, fix the payload and retry; do not invent alternate storage paths.
- Reuse `prepare_review`'s `diff-tmp` / hash unless the PR changed; then call `prepare_review` again.

## Queue / triage helpers (optional)

Before reviewing, you may use: `priority_prs`, `low_hanging_fruit`, `my_review_debt`,
`prs_blocked`, `prs_stale`, `pr_diff_stats`, `diff_hotspots` — then run the workflow above
on the chosen PR.

## Anti-patterns

- Do not spawn `claude` / `codex` / Desktop AI Hub agents for this flow.
- Do not skip `prepare_review` and invent a `diff_hash`.
- Do not upload with a stale hash after the PR diff changed.
- Do not put secrets or local file dumps into sidecars beyond what the schemas ask for.
