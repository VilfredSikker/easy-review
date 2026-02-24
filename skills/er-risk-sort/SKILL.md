# er-risk-sort

Re-sort the review order in `.er-order.json` based on risk analysis from `.er-review.json`.

## Trigger

Run as `/er-risk-sort`.

## What it does

1. Reads `.er-review.json` for per-file risk levels and findings
2. Reads `.er-order.json` (or creates it fresh if missing)
3. Sorts files by: high-risk first, then by number of findings, then by logical grouping
4. Groups related files together (e.g., a module and its tests)
5. Writes updated `.er-order.json`

## Sorting strategy

1. **Risk-first**: high → medium → low → info
2. **Within same risk**: more findings first
3. **Group adjacency**: keep related files together even if different risk levels
   - A test file should follow its implementation file
   - Config changes should be near the code that uses them
4. **Logical flow**: if file A calls file B, review A first

## Output

Updated `.er-order.json` with:
- `order` array sorted by the above criteria
- `groups` map with meaningful labels (e.g., "Core Logic", "API Layer", "Tests", "Config")
- Each group gets a color: red (high-risk group), yellow (medium), green (low), blue (info)

## Guidelines

- Read the actual code to understand relationships between files, don't just sort by filename
- Keep the group count reasonable (3-6 groups for most PRs)
- The `reason` field should tell the reviewer *why* this file matters in context
