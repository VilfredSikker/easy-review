# Co-located tests in the Guide

## What changed

The Guide (the AI guided walkthrough — `tour.json`, internally `DiffMode::Tour`)
now nests a file's **co-located related files** — its tests, styles, stories,
and snapshots — directly beneath it instead of scattering them across pillars or
dumping them in "Other changes".

When `er-tour` groups the diff into pillars, a changed file's accompanying
test/style/story/snapshot in the same diff is attached to that file rather than
listed as its own entry. In the pillar rail you now see, for example:

```
projections.ts
  ↳ projections.test.ts   test
filters.ts
  ↳ filters.test.ts       test
```

so a reviewer reads each file together with the tests that exercise it.

## How it works

- **Tour model.** `TourFile` gains an optional `related: [{path, kind, reason}]`
  array (`kind` ∈ `test`/`style`/`story`/`snapshot`/`other`). It is
  serde-defaulted, so older `tour.json` files keep loading unchanged.
- **Generation.** The `er-tour` skill and the desktop "Generate tour" prompt now
  instruct the model to put a file's test/style/story/snapshot into that file's
  `related` array instead of a standalone `files` entry. Each changed file still
  appears exactly once — as a primary entry or as one file's related child.
- **Rendering.** Related files render as indented child rows (a `↳` marker plus a
  small `kind` label) under their parent in both the desktop Guide rail
  (`PillarRail`) and the TUI pillar list. They are real diff files: still
  navigable, still independently markable as reviewed, and counted in the
  pillar's reviewed progress and "Review all".
- **Scope.** Only related files **present in the current diff** are shown
  (matching how the Guide already drops files absent from the diff). A related
  file is owned by its source file and is excluded from the "Other changes"
  bucket.

## Why it's safe

`related` defaults to empty, so a tour generated before this change renders
exactly as before. Co-located files were always part of the branch diff and the
branch review bucket; nesting only changes how they are grouped and displayed —
findings, comments, questions, and reviewed state remain keyed by file path and
shared across the Diff and Guide views.
