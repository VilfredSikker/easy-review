# Spike: [@pierre/trees](https://trees.software/) (read-only)

**Status:** exploratory — not wired into production `FileTree.svelte`.

## Goal

Evaluate trees.software for **read-only** diff review UX:

- Built-in virtualization + `prepareFileTreeInput`
- Git status colors (added / modified / deleted / …)
- File-type icons (`complete` set)
- In-tree search (`hide-non-matches`)
- Row decorations for er-specific +/−, findings, lazy stub
- **No** rename, drag-and-drop, or context menu

## How to run

```bash
cd desktop-ui
bun run storybook
```

Open **Spikes → TreesSoftware**:

| Story | Purpose |
|-------|---------|
| Default | Normal PR-sized list |
| RichPr | Findings + comments + mixed statuses |
| LargeRepo | ~1.2k paths, prepared input + scroll perf |
| CompareWithEr | Side-by-side with current er tree; use **Simulate watch** to bump +/− |

## Files

| File | Role |
|------|------|
| `treesFileTree.ts` | Map `FileSnapshot` → git status, decorations, host theme |
| `TreesFileTreeSpike.svelte` | Vanilla `FileTree` lifecycle in Svelte 5 |
| `TreesFileTreeSpikeHarness.svelte` | Storybook shell + live stat bump |
| `../stories/TreesFileTreeSpike.stories.ts` | Stories |

## Read-only configuration

- `composition.contextMenu.enabled: false`
- `dragAndDrop` / `renaming` omitted (defaults off)
- Selection via `onSelectionChange` only

Live updates use `setGitStatus` + `resetPaths` with fresh `preparedInput` (see harness **Simulate watch**).

## Observations (fill in after review)

| Area | Notes |
|------|-------|
| Theming | Shadow DOM + `themeToTreeStyles`; needs token pass for er semantic colors |
| er metadata | `renderRowDecoration` covers +/−; git lane separate from findings/comments |
| Selection | Path-first; maps cleanly to `select_file` / `source_index` lookup |
| Search | Built-in; may overlap with existing `/` filter bar |
| Bundle | `@pierre/trees@1.0.0-beta.4` devDependency only |
| Risk | Beta API; Svelte adapter is DIY (vanilla mount) |

## Recommendation

**Do not adopt for production.** er `FileTree` uses the classic layout (git status SVGs, `›` folder breadcrumbs, sparkle findings, collapsible folders) and fresh +/− via `resolveTreeFile`. Use this spike only as a scroll/icon benchmark.
