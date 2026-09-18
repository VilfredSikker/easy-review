# Desktop UI Agent Guide

`desktop-ui` consumes the Rust snapshot contract and owns only ephemeral browser/UI state. Rules and traps below; the tree answers everything else.

## The snapshot is pushed

The backend emits `er://revision`; the frontend polls in response and coalesces concurrent polls. A 30s timer covers only events fired before the listener attached. Shortening that interval will not make the UI fresher — a stale view means the backend failed to emit a revision. See `docs/adr/0011-push-revision-not-polling.md`.

Mutations go through `app.cmd`, which invokes, ingests the returned snapshot, and routes failures to the error banner, the toast and the log. Raw `invoke` is for returns that must not replace the live snapshot: a non-snapshot return (export string, provider list, terminal write), or a snapshot the caller ingests itself. See `docs/adr/0018-app-cmd-ingests-snapshots.md`.

## Differential snapshots

The backend omits hunks the frontend already holds (`hunks_omitted` + matching `delta_key`). See `docs/adr/0012-differential-snapshots.md`.

- `resolveOmittedHunks` runs on every snapshot that replaces `app.snapshot` wholesale, before it is stored. A stored snapshot never carries `hunks_omitted`.
- No matching previous content (first load, races, dropped snapshots) → the file becomes a lazy stub and the viewport loader re-fetches it. That is the recovery path; do not add a second one.
- Anything handing file content to the frontend calls `record_sent_file`; a from-scratch rebuild resets the map. Mis-recording costs a stub re-fetch or a redundant resend, and correctness is unaffected. Keep it that way.

## Content and chrome revisions

`content_revision` and `chrome_revision` are independent; `reviewed_revision` is deliberately outside both. See `docs/adr/0016-split-content-and-chrome-revisions.md`.

- Apply when either of the first two changes. A `chrome_only` response merges over the hunks and spans already held rather than replacing them — one combined counter would make every checkmark on a reviewed file pay to rebuild the diff.
- A chrome-only response may merge only onto the same view identity. Built for a different view (Branch vs PR Diff) it carries empty `files`, so defer it and wait for the full snapshot; merging leaves the previous view's diff on screen under the new tab.

## Tab cache

`tabCache` keeps a full snapshot per tab for instant revisit. It is the sanctioned exception to snapshot truth — the only one; do not grow another.

- Keyed by tab identity (`idx`, repo root, kind, branch, PR number) plus `change_token`. A moved token evicts the entry; `idx` alone is not identity, since closing a tab compacts it. First-select stubs have empty files and are never cached.
- Chrome is taken live on paint (tabs, projects, panels, theme, inbox), hunks from the cache. The inbox is global state that may be minutes old; repainting its items flashes cleared notifications back onto the screen.
- Race guards, both deliberate: a command snapshot whose tab key differs from the painted one is dropped, and a poll captured before `ingestCommandSnapshot` bumped the generation is discarded. A cache-hit paint of tab B can race a poll still built from tab A — the poll loses, and `select_tab`'s ingest applies B.

## Rules with consequences

- LocalStorage holds frontend preferences only: diff view mode, compact/wrap lines, comment visibility, drawer and rail sizes, section order. Scroll position is in-memory, keyed by diff mode, and dies with the window — it is persisted nowhere.
- `DiffView` renders many files and rows: windowed rendering, stable keys, measured placeholder heights, no per-scroll backend calls.
- Browser annotations post messages often. Persist committed annotations and re-anchor results; never persist every hover.
- Register keyboard shortcuts centrally in `keyboard.ts`. Component-local text inputs stop propagation themselves.
- Colors flow through the `@theme` tokens in `app.css`, overridden per theme by `themes.ts`. Fixed hex survives in exactly two places, both surfaces that are not ours to theme: `AppMark.svelte` (the brand mark) and the browser iframe ground (`bg-white` in `BrowserView.svelte`). The arena palette is not an exception — `--arena-*` aliases the theme tokens.
- Preserve the dense review UI unless the task asks for a redesign.
