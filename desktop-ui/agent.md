# Desktop UI Agent Guide

`desktop-ui` reads the backend's resources and owns only ephemeral browser/UI state. Rules and traps below; the tree answers everything else.

## Resources are pushed

The backend pushes `er://resources` with the keys whose version moved. The resource store holds each shown key at the version it fetched and refetches only when a push, an `Ack` or a version check names a higher one. It checks the versions once after its listener attaches, since a push sent before that is lost, and again on a 30s safety net. Shortening that interval will not make the UI fresher — a stale view means a writer that did not stamp a version, or a relay that did not hand it on. See `docs/adr/0072-the-desktop-protocol-is-versioned-resources.md`.

Commands go through `er_send` and answer an `Ack` that names the versions they moved. The store refetches those as soon as the `Ack` arrives, so a click's repaint never waits on the push. Every failure is an `ErError`: branch on its `kind`, never on its message.

## Hunks are content-addressed

A file's hunks are keyed by the hash of its own lines and never change, so they carry no version, and the hunk cache holds them by hash; revisiting a tab paints from it. A request for a hash the diff has moved past answers `stale`, and the loader refetches the diff rather than retrying the hash.

## Wire keys are never stored

Tab keys count from 1 in each backend process, so a key kept across a restart can name a different tab. A source scan (`resourceScans.test.ts`) keeps every module that holds a wire key away from `localStorage` and `sessionStorage`.

## Rules with consequences

- LocalStorage holds frontend preferences only: diff view mode, compact/wrap lines, comment visibility, drawer and rail sizes, section order. Scroll position is in-memory, keyed by diff mode, and dies with the window — it is persisted nowhere.
- `DiffView` renders many files and rows: windowed rendering, stable keys, measured placeholder heights, no per-scroll backend calls.
- Browser annotations post messages often. Persist committed annotations and re-anchor results; never persist every hover.
- Register keyboard shortcuts centrally in `keyboard.ts`. Component-local text inputs stop propagation themselves.
- Colors flow through the `@theme` tokens in `app.css`, overridden per theme by `themes.ts`. Fixed hex survives in exactly two places, both surfaces that are not ours to theme: `AppMark.svelte` (the brand mark) and the browser iframe ground (`bg-white` in `BrowserView.svelte`). The arena palette is not an exception — `--arena-*` aliases the theme tokens.
- Preserve the dense review UI unless the task asks for a redesign.
