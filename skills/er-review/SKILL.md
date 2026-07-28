---
name: er-review
description: >
  Run Easy Review PR reviews via the er-mcp tools — triage, full review, or guided
  tour — then upload sidecars into shared Desktop/TUI storage. Use when the user says
  "ER review", "easy review", "triage this PR", "upload review artifacts", or asks to
  prepare/review a GitHub PR with Easy Review MCP.
metadata:
  author: easy-review
  version: "0.1.1"
---

# Easy Review (er-mcp)

You are the reviewer. Easy Review MCP prepares storage and validates uploads — it does
**not** spawn agent CLIs. Sidecars land in the same managed path Desktop/TUI read:

`~/Library/Application Support/easy-review/repos/<owner-repo>/prs/pr-<N>/`  (macOS)
`~/.local/share/easy-review/repos/<owner-repo>/prs/pr-<N>/`                 (Linux)

This directory is **read-only from the sandbox** — read `diff-tmp` from it, write nothing into it.

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

1. **`prepare_review`** with `{ "number": N, "kinds": [...], "repo": "owner/name" }` when needed.
   - Writes shared `diff-tmp` under the managed PR bucket.
   - Returns `diff_hash`, `diff_tmp_path`, and `artifact_specs` — the **same** schemas/examples/prompts
     `get_artifact_specs` returns. Do **not** call both; that duplicates a multi-thousand-token payload.
   - Call `get_artifact_specs` alone only when you are not preparing a PR (authoring offline, inspecting a schema).
2. **Read the prepared diff** at `diff_tmp_path` (and follow the prepared-diff prompts).
3. **Author sidecar JSON/Markdown** yourself:
   - Embed the **exact** `diff_hash` from `prepare_review` in every JSON that requires it.
   - Follow schemas/examples from `artifact_specs` (or `get_artifact_specs` when used alone).
4. **`upload_artifacts`** once per kind:
   - Triage: `{ "kind": "triage", "files": { "triage.json": "..." } }`
   - Tour: `{ "kind": "tour", "files": { "tour.json": "..." } }`
   - Review: all four — `review.json`, `order.json`, `checklist.json`, `summary.md`
5. **`pin_pr`** (optional but recommended) so the PR lands in Desktop Saved and is easy to find later via `list_pinned_prs`.
6. **`summarize_triage`** (optional) and/or **`open_in_easy_review`** so the user can open Desktop/TUI.

Do **not** write files under the managed path yourself — always go through `upload_artifacts`.
Do **not** auto-pin — only call `pin_pr` when the user wants it bookmarked, or after a successful review when they asked to save/pin.

## Local working rules

- **Annotate into the scratchpad**, not the managed dir: redirect the spec's `awk` to
  `$SCRATCHPAD/diff-annotated`, then read it from there.
- **Author each sidecar once**, inline in the `upload_artifacts` call. `upload_artifacts` takes file
  *contents*, so a scratchpad copy means writing the same JSON twice — and the upload is the durable
  artifact. Use a scratchpad draft only if you expect validation to fail and want to iterate.
- Use `kit.diff_hash` / `prepare_review`'s `diff_hash` verbatim; skip the spec prompt's step-1 `shasum`.

## Finding reviewed work

- **`list_pinned_prs`** — Desktop Saved PRs (explicit pins), with sidecar presence.
- **`list_artifacts`** — scan managed storage for any uploaded triage/review/tour (pinned or not).
- **`unpin_pr`** — remove from Desktop Saved.

## Validation rules

- `upload_artifacts` checks serde shape + matching `diff_hash` **before** writing.
- It does **not** enforce full JSON Schema — treat `prepare_review`'s `artifact_specs` as the
  authoring contract (or `get_artifact_specs` when used alone).
- If upload fails, fix the payload and retry; do not invent alternate storage paths.
- Reuse `prepare_review`'s `diff-tmp` / hash unless the PR changed; then call `prepare_review` again.

## Queue / triage helpers (optional)

Before reviewing, you may use: `priority_prs`, `low_hanging_fruit`, `my_review_debt`,
`prs_blocked`, `prs_stale`, `pr_diff_stats`, `diff_hotspots` — then run the workflow above
on the chosen PR.

## Anti-patterns

- Do not spawn `claude` / `codex` / Desktop AI Hub agents for this flow.
- Do not skip `prepare_review` and invent a `diff_hash`.
- Do not call both `get_artifact_specs` and `prepare_review` in the same review run.
- Do not upload with a stale hash after the PR diff changed.
- Do not put secrets or local file dumps into sidecars beyond what the schemas ask for.
