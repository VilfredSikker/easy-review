# er-wizard

Generate a guided tour of the important changes in the current diff, writing `.er/wizard.json` for the `er` TUI's Wizard mode.

## Trigger

Run as `/er-wizard` or `/er-wizard [scope] [base-branch]`.

## Arguments

Same as `/er-review`:
- `branch` or `1` (default) — full branch diff
- `unstaged` or `2` — uncommitted changes
- `staged` or `3` — staged changes

Optional base branch follows the scope: `/er-wizard branch develop`.

## GitButler awareness

Before Step 1, check if `.er/gb-context.json` exists (Read tool). If it exists and `enabled` is true:

1. Extract `binary`, `selected_branch_id`, and `selected_branch` from the JSON
2. Set `ER_DIR` to `.er/stacks/<selected_branch>/` (create with `mkdir -p`)
3. For the diff capture, use:
   ```
   scripts/er-gb-diff <binary> <selected_branch_id> <ER_DIR>/diff-tmp
   ```
   instead of `git diff <base> ...`. The helper script handles JSON parsing, unified diff reconstruction, shasum, and HEAD output. It matches the allowed `scripts/er-*` pattern.
4. All `.er/` file reads and writes in this skill use `<ER_DIR>/` instead of `.er/`
   (e.g., `<ER_DIR>/wizard.json` instead of `.er/wizard.json`)
5. The persistence cache path becomes `<ER_DIR>/reviews/<branch>/<commit>/`
6. For `diff_hash`, hash the `<ER_DIR>/diff-tmp` file as usual
7. Set `base_branch` in output JSON to the GitButler target branch (from `<binary> config --json`)

If `.er/gb-context.json` does not exist, proceed with the normal git diff flow (backward compatible).

**Permission note:** The GitButler binary path (e.g., `/Applications/GitButler.app/Contents/MacOS/gitbutler-tauri`) is allowed as a first-word command for Bash calls.

## What it does

1. Reads the current diff (same command as `/er-review`)
2. Optionally reads `.er/review.json` if it exists (for additional context — not required)
3. Identifies the fundamental and important changes, their relationships, and logical groupings
4. Writes `.er/wizard.json` with a tour ordering

## Purpose — Tour, Not Review

The wizard is a **guided tour of what changed and why**, NOT a list of problems. Think of it as a knowledgeable colleague walking you through the PR:

- "Here's the core change — we rewrote the auth module to use RS256"
- "This file supports that — it updates the middleware to pass the new key format"
- "These test files verify the new behavior"

The wizard answers **"what happened here?"** while the review answers **"what could go wrong?"**.

## Importance levels

Each file in the tour gets an importance level:

| Level | Meaning | When to use |
|-------|---------|-------------|
| `fundamental` | Core change — the reason this PR exists | New features, architectural changes, algorithm rewrites |
| `important` | Directly supports or enables the fundamental changes | Adapters, middleware updates, config changes needed by the core |
| `supporting` | Completes the picture but not essential to understand first | Tests, docs, minor adjustments, dependency bumps |

## Speed budget

**Target: ≤6 tool calls, ≤45 seconds.**

- TOOL CALL 0: Read `.er/gb-context.json` (GB check — skip if missing; sets `ER_DIR`)
- TOOL CALL 1: Bash — capture diff + hash (GB mode: `<binary> diff`; normal: same as er-review step 1)
- TOOL CALL 2: Read `<ER_DIR>/review.json` (optional — skip if missing)
- TOOL CALL 3: Read `<ER_DIR>/diff-tmp` (full diff into context)
- IN-CONTEXT: Analyze changes and build tour — zero tool calls
- TOOL CALL 4: Write `<ER_DIR>/wizard.json`

`<ER_DIR>` is `.er/` normally, or `.er/stacks/<branch>/` in GitButler mode.

### Permission & hook constraints

All Bash commands MUST start with an allowed command: `git`, `shasum`, `cp`, `mkdir`, `scripts/er-*`, and the GitButler binary path when in GB mode.
Do NOT pipe (`|`) into `shasum`. Do NOT chain `rm` with `&&`.

## Tour design rules

- **Include every file that matters** — don't skip files just because they're small
- **Order by understanding flow** — fundamental changes first, then their dependencies, then supporting files
- **Group related files** — if 3 files work together for one feature, put them adjacent in the tour
- **Write clear summaries** — each summary should explain what changed AND why it matters
- **Key changes as bullets** — concrete, specific changes (not vague descriptions)
- **Link related files** — if file A calls into file B, note that relationship

## Output schema

### `.er/wizard.json`

```json
{
  "version": 1,
  "diff_hash": "<sha256>",
  "tour": [
    {
      "path": "src/auth.rs",
      "importance": "fundamental",
      "summary": "Rewrites JWT handling from HS256 to RS256, separating signing and verification keys. This is the core change that motivated the PR.",
      "key_changes": [
        "Algorithm switch from HS256 to RS256 in token creation",
        "New key loading from PEM files instead of shared secret",
        "Token refresh now propagates errors instead of swallowing them"
      ],
      "related_files": ["src/middleware.rs", "src/config.rs"]
    },
    {
      "path": "src/middleware.rs",
      "importance": "important",
      "summary": "Updates request authentication middleware to use the new public key for verification instead of the shared secret.",
      "key_changes": [
        "Verification uses public key from config instead of shared HMAC secret",
        "Error handling updated to match new error types from auth.rs"
      ],
      "related_files": ["src/auth.rs"]
    },
    {
      "path": "tests/auth_test.rs",
      "importance": "supporting",
      "summary": "New test coverage for RS256 token flow.",
      "key_changes": [
        "Tests for key pair generation and loading",
        "Integration test for full sign-verify cycle"
      ],
      "related_files": ["src/auth.rs"]
    }
  ]
}
```

**Fields:**
- `version` — always `1`
- `diff_hash` — SHA-256 of the raw diff (for staleness detection)
- `tour` — ordered array of files to visit
  - `path` — file path relative to repo root
  - `importance` — `"fundamental"`, `"important"`, or `"supporting"`
  - `summary` — what changed and why (1-3 sentences)
  - `key_changes` — bullet list of specific changes in this file
  - `related_files` — paths to files that interact with this one

## Guidelines

- Every tour entry should be anchored to actual diff content — no speculative entries for unchanged files
- Summaries should teach the reader what to look for, not just describe what lines changed
- If the diff is trivial (only cosmetic/formatting changes), produce a minimal tour or print "No significant changes to tour"
- Files not included in the tour will still be accessible in wizard mode but shown after the tour entries
- If `.er/review.json` exists, you may reference its findings for context, but the tour should focus on understanding, not problems
