# er-arch-map: deterministic architecture extractor

Status: in-progress

`er-arch-extract.mjs` reads a git repo (plus an optional `architecture.json` model)
and writes `<repo>.er-arch.json` (format `er-arch/1`), which `er-map.html` imports.
No dependencies; Node 18+ or Bun. Same input gives the same output apart from `generatedAt`.

    node er-arch-extract.mjs <repo> -o out.er-arch.json [--model architecture.json] [--base <ref>] [--timings]

## Open fixes (from the ink-booking review)

1. Files run by a `package.json` script count as entry points (tooling when seed/build/gen).
   Case: `supabase/seed-storage.ts` reported unused.
2. Follow `import.meta.glob(...)` patterns as imports.
   Case: ~20 `src/lib/server/mcp/outputs/*.ts` reported test-only.
3. Categorise by file name: `*-fixtures.*`, `*-test-utils.*`, `*.fixtures.*` as test;
   `*-story-data.*` as dev. Case: fixture/test-util files inside `src/lib`.
4. Check `*.types.svelte` imports (`<script module>` exports?) before calling them unused.
5. ink-booking was dirty but `change` was empty: confirm only non-code files changed.

## Baseline

`baselines/ink-booking.2b0fb0bca1.er-arch.json` is the user's run before these fixes.
Diff new output against it; every change should be explained by a fix above.

## Lessons

- Avoid giant regex alternations: V8 and JavaScriptCore differ wildly. Split words and look up in a Set.
- Test status propagates only from real test code, never from generated or tooling files.
