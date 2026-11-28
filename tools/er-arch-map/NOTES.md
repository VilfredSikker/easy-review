# er-arch-map: deterministic architecture extractor

Status: in-progress

`er-arch-extract.mjs` reads a git repo (plus an optional `architecture.json` model)
and writes `<repo>.er-arch.json` (format `er-arch/1`), which `er-map.html` imports.
No dependencies; Node 18+ or Bun. Same input gives the same output apart from `generatedAt`.

    node er-arch-extract.mjs <repo> -o out.er-arch.json [--model architecture.json] [--base <ref>] [--timings]

## Who finds the patterns

The extractor writes facts only: files, categories, edges, outside systems, entries, parts, tables. er Map
(`er-map.html`) labels the patterns (layered, service layer, hub, facade, ports, tangles) in the browser
from those facts, in `patterns()` and `proposals()`.

## How er Map reads the output

A file is **unused** when nothing imports it and it is not an entry point. It is
**test-only** when only test, story or tooling code imports it. Every edge from a
categorised file (test, dev, generated, tooling) counts as non-functional. So a
wrong category on an importer makes everything it imports look test-only.

## Fixed in 2026.11.28 (from the ink-booking review)

1. Every code file a `package.json` script names is an entry point. It is also
   tooling when a runner word (node, bun, tsx, deno, …) earlier in the same command
   executes it and a word of the script name is a build step (gen, build, seed,
   lint, check, …). So `bun run supabase/seed-storage.ts` under `db:seed-storage`
   is tooling, while `tsup src/index.ts` under `build` stays product code.
2. `src/lib/server/mcp/outputs/*.ts` were not loaded by a glob. `mcp/tools/*.ts`
   imports them, and the `tools/` folder name made those importers tooling. A
   `scripts/`, `tools/`, `tooling/` or `ci/` folder now counts as tooling only
   when no `src/` folder sits above it. `import.meta.glob` is followed too
   (`./`, `../`, `/` as the Vite root, arrays, `!` negations, `{a,b}`), but
   aliases such as `$lib` are not.
3. `*-fixtures.*`, `*.fixtures.*`, `*-test-utils.*` and `*.types.svelte` are
   test; `*-story-data.*` is dev. `*.types.svelte` files are type fixtures that
   svelte-check compiles and nothing renders (ink-booking's knip config lists
   them as entries), which makes them type tests. Only `.svelte`:
   `database.types.ts` is imported production code.
4. SvelteKit layout resets (`+page@.svelte`, `+page@(group).svelte`) are
   entry points.

## Added in 2026.11.28.2

- SvelteKit apps (a `svelte.config` beside `src/routes`) get one inferred part per route group and per
  `src/lib/server` folder. A group's id ends in `-group`, so `src/routes/(auth)` and `src/routes/auth`
  stay apart.
- knip's `entry` globs (`knip.json`, `knip.jsonc`, package.json `"knip"`, and each workspace's) are entry points.
- `tables`: `[file, name, read | write | call | other, line, test]` for supabase-js `.from('t')` and
  `.rpc('fn')`, and a `net` edge to Supabase with the name as its symbol. Only when a package.json
  depends on `@supabase/supabase-js` or `@supabase/ssr`; when the repo has SQL, only names it creates.
  `storage.from`, `Buffer.from` and similar are skipped. er Map ignores the `tables` list.
- `change.deps` symbols are the ones whose count moved, with the delta, so a new table read shows by name.

## Checking er Map

`er-map.html` loads `data.js` (its built-in snapshots), which is not committed, so the page fails on its
own. A headless run serves an empty `data.js`, calls `useImport(text, false)` and `loadSnap(key)`, then
reads `patterns(view(level))` for each of `sysLevels()` and `buildRecs()`. Use easy-review with its
model as the control.

Fixed: a `rules.ports` key `service:<name>` now maps to `x-svc-<name>`; before, the rule never matched.

Open, found on ink-booking (tuned on easy-review):
- The service-layer check counts every front-end reference that does not reach the service, including
  imports of shared UI and foundation code.
- The facade check wants one entry per subsystem, even where the docs name several.
- The bridge check counts SDK calls (Supabase) as raw network access.
- Merge suggestions ignore declared boundaries.
- "Send outside calls through their port" shows the service-layer evidence as its reason.
- Part names and texts from easy-review are hardcoded: "Goes around er-service", "plan §3.1", `protocol`, `layer === 3`.
- Type-only imports count like runtime imports, in the extractor and the map.

## Known gaps

- A file loaded by a path string, not an import, still looks test-only. Case:
  ink-booking's `storybook-gate.ts`, which `scripts/storybook-deploy.ts` deploys
  as an Edge function.
- `test-data.ts` and other test support files with no fixture or test-utils
  suffix still count as production code.

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
