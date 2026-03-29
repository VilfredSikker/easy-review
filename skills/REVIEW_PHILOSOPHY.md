# Review Philosophy

Shared reference for all `er-*` skills. Every skill that produces findings, checklist items, quiz questions, or summaries MUST follow these rules.

## Severity Model — P0 / P1 / P2

Severity maps to semver impact:

| Level | Semver | Meaning | Examples |
|-------|--------|---------|---------|
| **P0** | Major | Breaks things or creates security risk. Must fix before merge. | Auth bypass, data loss, broken API contract, panic/crash path, SQL injection |
| **P1** | Minor | Observable gap: missing edge case, logic error, test gap. Should fix. | Missing error branch, incorrect calculation, untested failure path, stale mock |
| **P2** | Patch | Minor quality issue. Nice to fix, not blocking. | Redundant allocation, minor inefficiency, unclear variable name (when it causes confusion) |

**Mapping to `er` risk levels:**
- `high` → P0
- `medium` → P1
- `low` → P2
- `info` → observation only (no action required)

## What NOT to Flag

Never produce a finding, checklist item, or quiz question about:

- **Naming** — variable names, function names, file names (unless the name is actively misleading about behavior)
- **Formatting** — whitespace, indentation, line length, trailing commas
- **Style** — code style preferences, idiomatic rewrites that don't change behavior
- **File moves / renames** — restructuring without logic changes
- **Import organization** — reordering imports, grouping use statements
- **Comment quality** — missing comments, comment phrasing (unless a comment is factually wrong)
- **`info`-only files** — files that only get `risk: "info"` and have no findings should not generate checklist items or quiz questions

If you're uncertain whether something belongs on this list, ask: "Does this affect correctness, security, or reliability?" If no, don't flag it.

## Test Quality Validation (P1 Focus)

Tests that only check existence are a P1 finding:

```rust
// P1 — this test proves nothing
assert!(result.is_some());
assert!(element.is_truthy());
expect(result).toBeDefined();

// Good — verifies actual behavior
assert_eq!(result.unwrap().status, Status::Active);
assert_eq!(error.message, "invalid token");
```

When reviewing test files, check:
1. Do assertions verify actual values, not just existence?
2. Are there negative tests (what should NOT happen)?
3. Do integration tests hit real dependencies, or are they mocked in ways that diverge from production behavior?

Flag shallow tests as **P1** with a specific suggestion for what to assert instead.

## Finding Quality Rules

- **Be specific**: "Handle `None` from `parse_token()` at line 42" — not "check error handling"
- **Pin to hunks**: always set `hunk_index` (0-based). Pick the most relevant hunk if a finding spans multiple.
- **Cap findings**: max 3 per file, max 15 total. Prioritize P0 > P1 > P2.
- **Actionable suggestions**: the `suggestion` field says what to change, not just what's wrong.
- **Short titles**: max 60 characters (rendered inline in the TUI).

## Summary and Checklist Rules

- **Summaries** focus on breaking changes, logic changes, security implications. Don't mention cosmetic changes unless they're the only changes.
- **Checklist items** are for P0/P1 concerns only. Each item is something a human must manually verify — not something Claude already checked. Include at least one test-quality item when tests are modified.
- **No checklist items** about naming, formatting, or style.

## Quiz Rules

- Questions only about P0/P1 changes.
- Categories: `breaking-changes`, `security`, `data-integrity`, `logic-paths`, `error-handling`, `test-quality`.
- Never ask about naming, formatting, or cosmetic decisions.
