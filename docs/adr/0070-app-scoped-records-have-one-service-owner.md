# App-scoped records have one service-level owner

## Context

ADR 0067 gives each repo or worktree its own Workspace actor. `tabs.json` and
`pr-open-cache.json` cover the whole app, so an actor per workspace that saved
them would make every open workspace a writer of the same file.

## Decision

The service creates one `AppRegistry` per process
(`crates/er-service/src/app_registry.rs`) and keeps it in a fixed slot, never
picked from a tab. It owns the tab list, `tabs.json`, the PR open cache and its
file, and the in-memory remote open cache. Every save of either file goes
through it; workspace actors reach it through `WorkspaceDeps` and only read or
send it commands. Per-repo state stays with the Workspace actor. Phase 6's
inbox and project records are app-scoped too and follow the same home.

Its locks cover in-memory changes only: a save snapshots the state under the
lock and writes after releasing it, and port calls run on effect threads.

## Consequences

- The tab record lives in the engine, which must not name the desktop's
  browser fields (ADR 0068). `TabDescriptor` keeps keys it does not own in a
  flattened map, and the desktop reads and writes its browser URL and layout
  there. A key the engine does not know now survives a save, and the saved
  file's key order can differ from older builds.
- Tab commands name a tab by its `TabKey`; a position travels only where the
  command needs one, as the reorder target.
