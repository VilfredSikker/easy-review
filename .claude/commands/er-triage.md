# er-triage

Fast branch-wide scan — first impression, routing verdict, and priority files. Writes **only** `.er/triage.json`.

## Trigger

`/er-triage` or `/er-triage [scope] [base-branch]`

Scopes: `branch` (default), `unstaged`, `staged` — same as `/er-review`.

## Philosophy

**Breadth over depth.** You are not doing a full review. Scan every changed file at file/hunk-header level, classify domains, and tell the human what to run next.

## Diff

Two-dot `git diff <base> --unified=20 --no-color --no-ext-diff` → `.er/diff-tmp`, hash once.

Do **not** write `review.json`, `order.json`, `checklist.json`, or `summary.md`.

## Verdict rules

| `verdict.primary` | When |
|-------------------|------|
| `skip` | Cosmetic-only, docs-only, lockfiles, formatting — no logic to review |
| `general` | Mixed concerns; no single dominant expert lens |
| `expert` | Clear dominant domain — set `verdict.experts` to one or more: `security`, `performance`, `reliability`, `testing`, `api`, `patterns`, `simplifying`, `mentorship` |
| `arena` | Large/high-stakes diff, conflicting signals, or needs multi-model second opinion |
| `professor` | Novel subsystem the reader should learn before reviewing for bugs |

## Output

`mkdir -p .er` then write **only** `.er/triage.json`:

```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "diff_scope": "<scope>",
  "created_at": "<ISO8601>",
  "first_impression": "2–4 short markdown paragraphs",
  "diff_stats": {
    "files_changed": 0,
    "approx_risk": "low|medium|high",
    "domains": ["auth", "api"]
  },
  "verdict": {
    "primary": "general|expert|arena|professor|skip",
    "experts": ["security"],
    "rationale": "Why this next step",
    "confidence": "high|medium|low"
  },
  "priority_files": [
    { "path": "src/auth.rs", "reason": "New trust boundary", "risk": "high" }
  ]
}
```

## Speed budget

**Target: ≤8 tool calls, <60 seconds.**

- Read diff once in context
- No per-file deep reads unless a path is ambiguous from headers alone
- Up to **12** `priority_files`, ranked by review urgency
- At most **2** informational flags total (optional) — verdict + priority list carry the signal

## Model

Prefer a fast/small model (e.g. Haiku). `er` spawns triage with `[ai_hub.reviewer_models].triage` when configured.
