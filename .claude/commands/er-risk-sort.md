# er-risk-sort

Re-sort the review order in `.er/order.json` based on risk analysis from `.er/review.json`.

## Trigger

Run as `/er-risk-sort`.

## What it does

1. Reads `.er/review.json` for per-file risk levels and findings
2. Reads `.er/order.json` (or creates it fresh if missing)
3. Sorts files by: high-risk first, then by number of findings, then by logical grouping
4. Groups related files together (e.g., a module and its tests)
5. Writes updated `.er/order.json`

## Speed budget

**Target: ≤4 tool calls, ≤30 seconds.**

- TOOL CALL 1: Read .er/review.json
- TOOL CALL 2: Read .er/order.json (if exists, to preserve structure)
- IN-CONTEXT: Sort and group using review data — zero tool calls
- TOOL CALL 3: Write .er/order.json

## Sorting strategy

Sort order: **P0 → P1 → P2 → cosmetic/info last**

1. **Risk-first**: high (P0) → medium (P1) → low (P2) → info
2. **Within same risk**: more findings first
3. **Cosmetic files last**: files with only renames, formatting changes, import reordering, or `risk: "info"` and zero findings go to the bottom of the order
4. **Group adjacency**: keep related files together even if different risk levels
   - A test file should follow its implementation file
   - Config changes should be near the code that uses them
5. **Logical flow**: if file A calls file B, review A first

## Output

Updated `.er/order.json` with:
- `order` array sorted by the above criteria
- `groups` map with meaningful labels (e.g., "Core Logic", "API Layer", "Tests", "Config")
- Each group gets a color: red (high/P0 group), yellow (medium/P1), green (low/P2), blue (info/cosmetic)

## Guidelines

- Use the review data (summaries, findings, related_files) to understand relationships — do NOT read source files
- Keep the group count reasonable (3-6 groups for most PRs)
- The `reason` field should tell the reviewer *why* this file matters in context
- Files with only renames or formatting (no findings, cosmetic `risk: "info"`) get reason: "Cosmetic only — review last"
- See `skills/REVIEW_PHILOSOPHY.md` for severity definitions
