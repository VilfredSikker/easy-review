# er-tour

Generate a guided **Tour** of the current git diff — an AI analysis that groups
the changed files into ordered **pillars** (foundation first, then importance),
each with a description and its relevant files. Writes a single `.er/tour.json`
sidecar read by the `er` TUI (Tour mode) and desktop (Guide tab).

> **Where the output lands.** This skill writes `.er/tour.json` in the repo root
> (like `er-review` and the other `er-*` skills). The app reads review artifacts
> from managed storage (`~/.local/share/easy-review/...`) by default, so a
> manually-run `/er-tour` surfaces in the app only when `ER_REPO_LOCAL=1` is set,
> or on first open before managed storage is populated. The reliable in-app path
> is the desktop **"Generate tour"** button, which runs this analysis and writes
> `tour.json` directly into the branch's managed bucket.

**Apply [`../REVIEW_RULES.md`](../REVIEW_RULES.md)** for the diff command,
anchors, and caps. This skill is additive: it writes **only** `.er/tour.json`
and never modifies `review.json`, `order.json`, or any other existing sidecar.

## Trigger

Run as `/er-tour` or `/er-tour [scope] [base-branch]`.

## Arguments

`/er-tour [scope]`

- `branch` or `1` (default) — full branch diff: `git diff <base> --unified=20 --no-color --no-ext-diff`
- `unstaged` or `2` — uncommitted changes: `git diff --unified=20 --no-color --no-ext-diff`
- `staged` or `3` — staged changes: `git diff --staged --unified=20 --no-color --no-ext-diff`

A base branch can optionally follow the scope: `/er-tour branch develop`. If no
base branch is given, detect main or master.

## Diff source — CRITICAL

Use the same diff command `er` uses internally, so the `diff_hash` matches and
the tour is not shown as stale:

```
git diff <base> --unified=20 --no-color --no-ext-diff
```

This is a **two-dot** diff (working tree vs base). Do NOT use three-dot
(`git diff main...HEAD`). For `unstaged`/`staged`, the base branch is irrelevant.

The `diff_hash` is the SHA-256 of the raw diff: `shasum -a 256 <file>` (never
pipe into `shasum` — use file-based hashing).

## What it does

1. Reads the diff for the selected scope and computes its SHA-256 `diff_hash`.
2. If a fresh `.er/review.json` exists (matching `diff_hash`), reads it as a
   starting point — reuse its file groupings and reference finding ids in
   `TourFile.finding_ids`. This is optional; the tour does not require a review.
3. Analyses the diff in a single in-context pass and groups files into pillars:
   - **Foundation first.** A pillar is `foundation: true` when other pillars
     build on it (data models, core types, shared utilities, schema). Order
     foundational pillars before the features that depend on them.
   - **Then importance.** Within/after foundation, order by reviewer attention
     (`importance` 0–100): risky/central changes before peripheral ones.
   - Each pillar gets a short `title`, a 1–3 sentence markdown `description`
     (what it is and why review it here), and its `files` in reading order with
     a one-line `reason` each.
   - Every changed file should appear in exactly one pillar. Group trivial
     files (lock files, generated, config) into a single low-importance pillar.
4. Writes `.er/tour.json` (atomic) and persists a cached copy at
   `.er/reviews/<branch>/<commit>/tour.json`.

## Model

When the desktop "Generate tour" button spawns this analysis, it defaults to a
**Sonnet-class model** (clustering + short descriptions don't need Opus). Override
per repo in `.er-config.toml`:

```toml
[ai_hub.reviewer_models]
tour = "claude-haiku-4-5"   # faster/cheaper, or any configured model id
```

## Speed budget

**Target: ≤8 tool calls, ≤60 seconds.** Read the diff into context once; do all
grouping in-context (zero tool calls). Write the output in one `Write` call.

### Permission & hook constraints

Allowed Bash first-words: `git`, `shasum`, `mkdir`, `cp`, `scripts/er-*`.
NOT allowed: `for`, `rm`, `while`, `bash`, `sh`. Chain with `&&`; do not pipe
into `shasum`.

## Step-by-step

```
TOOL CALL 1 — Bash (setup):
  For branch scope:  scripts/er-freshness-check.sh <base>   → "ok", hash, commit
  For unstaged/staged:
    git diff <scope-args> > .er/diff-tmp && shasum -a 256 .er/diff-tmp && git rev-parse --short HEAD && git branch --show-current

TOOL CALL 2 — Read .er/tour.json (if it exists):
  → If exists AND diff_hash matches → print "Tour is current", DONE.

TOOL CALL 3 — Read .er/diff-tmp (the full diff into context).

TOOL CALL 4 (optional) — Read .er/review.json (if fresh) for grouping hints + finding ids.

IN-CONTEXT ANALYSIS (zero tool calls):
  Build the pillars (foundation-first, then importance) as described above.

TOOL CALL 5 — Write .er/tour.json (schema below).

TOOL CALL 6 — Bash (persist):
  mkdir -p .er/reviews/<branch>/<commit>/ && cp .er/tour.json .er/reviews/<branch>/<commit>/

Print a one-line summary: "Tour: N pillars, M files".
```

## Output schema — `.er/tour.json`

```json
{
  "version": 1,
  "diff_hash": "<sha256 of raw diff>",
  "created_at": "<ISO 8601>",
  "title": "Tour: OAuth token refresh",
  "overview": "This change adds refresh-token handling across the auth stack.",
  "pillars": [
    {
      "id": "p-1",
      "title": "Foundation: token storage",
      "description": "Start here. The new TokenStore is the data model every other pillar builds on.",
      "order": 0,
      "importance": 90,
      "foundation": true,
      "files": [
        {"path": "src/auth/store.rs", "reason": "Defines TokenStore", "finding_ids": ["f-1"]}
      ]
    },
    {
      "id": "p-2",
      "title": "Refresh flow",
      "description": "The refresh path that consumes TokenStore.",
      "order": 1,
      "importance": 70,
      "foundation": false,
      "files": [
        {"path": "src/auth/refresh.rs", "reason": "Token refresh logic"}
      ]
    }
  ]
}
```

Field notes:
- `order` — lower sorts earlier. Foundational pillars sort before non-foundation
  at equal `order`; ties broken by descending `importance`.
- `importance` — 0–100 reviewer-attention weight.
- `foundation` — true when other pillars depend on this one.
- `files[].path` — the **new-side** path, matching the diff (`b/<path>`). Files
  not present in the current diff are skipped by the UI; unassigned diff files
  fall into an "Other changes" pillar automatically.
- `files[].finding_ids` — optional ids from `review.json` (omit if no review).

## Guidelines

- 3–7 pillars is the sweet spot. Don't make a pillar per file.
- Titles under ~40 characters (they render in the pillar nav).
- Descriptions explain *why review this here* and *what to look for*, not a
  restatement of the diff.
- Every changed file appears in exactly one pillar.

## .gitignore

`.er/` covers everything, including `.er/tour.json` and `.er/reviews/`.
