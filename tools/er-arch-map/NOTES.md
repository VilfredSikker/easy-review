# er-arch-map: deterministic architecture extractor

Status: in-progress

`er-arch-extract.mjs` reads a git repo (plus an optional `architecture.json` model)
and writes `<repo>.er-arch.json` (format `er-arch/1`), which `er-map.html` imports.
No dependencies; Node 18+ or Bun. Same input gives the same output apart from `generatedAt`.

    node er-arch-extract.mjs <repo> -o out.er-arch.json [--model architecture.json] [--base <ref>] [--timings]

## How er Map reads the output

A file is **unused** when nothing imports it and it is not an entry point. It is
**test-only** when only test, story or tooling code imports it. Every edge from a
categorised file (test, dev, generated, tooling) counts as non-functional. So a
wrong category on an importer makes everything it imports look test-only.

## Fixed in 2026.11.28 (from the ink-booking review)

1. Every code file a `package.json` script names is an entry point. It is also
   tooling when a word of the script name is a build step (gen, build, seed, lint,
   check, …). Commands are split into words, not matched per runner, so
   `bun run supabase/seed-storage.ts` under `db:seed-storage` now counts.
2. `src/lib/server/mcp/outputs/*.ts` were not loaded by a glob. `mcp/tools/*.ts`
   imports them, and the `tools/` folder name made those importers tooling. A
   `scripts/`, `tools/`, `tooling/` or `ci/` folder now counts as tooling only
   when no `src/` folder sits above it. `import.meta.glob` is followed too
   (`./`, `../`, `/` as the Vite root, arrays, `!` negations, `{a,b}`), but
   aliases such as `$lib` are not.
3. `*-fixtures.*`, `*.fixtures.*`, `*-test-utils.*` and `*.types.svelte` are
   test; `*-story-data.*` is dev. `*.types.svelte` files are type fixtures that
   svelte-check compiles and nothing renders (ink-booking's knip config lists
   them as entries), so they are type tests, not dead code. Only `.svelte`:
   `database.types.ts` is imported production code.
4. SvelteKit layout resets (`+page@.svelte`, `+page@(group).svelte`) are
   entry points.

## Known gaps

- A file loaded by a path string, not an import, still looks test-only. Case:
  ink-booking's `storybook-gate.ts`, which `scripts/storybook-deploy.ts` deploys
  as an Edge function.
- `test-data.ts` and other test support files with no fixture or test-utils
  suffix still count as production code.
- knip's `entry` globs would be a good source of entry points; not read yet.

## Checking a change

- Diff against the previous script's output on the same commit, and explain every
  changed file category, edge, entry and mention.
- `baselines/ink-booking.2b0fb0bca1.er-arch.json` came from an earlier script that
  also listed mentions for categorised files. Those 914 mention rows differ from
  any later run; everything else matches it at ink-booking `f213f4bec`.
- Run the regression on a clean clone, not this worktree: the script scans its own
  tree, so editing it makes the run dirty and adds a `change` block. The repo root
  has `architecture.json`, which loads without `--model`; use `main` for a run
  with no model.
- `repo.name` is the clone's folder name, so name scratch clones after the repo.

## Lessons

- Avoid giant regex alternations: V8 and JavaScriptCore differ wildly. Split words and look up in a Set.
- Test status propagates only from real test code, never from generated or tooling files.
- Check the claimed cause in the target repo before writing a fix. The glob theory
  for `mcp/outputs` was wrong; the importer's category was the cause.
