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
  there. A key the engine does not know now survives the engine's load and
  save, and so a save from the registry; the desktop's flag-off save, gone
  since phase 5, rebuilt each descriptor from its tab and dropped such a key. The saved file's
  key order can differ from older builds.
- Tab commands name a tab by its `TabKey`; a position travels only where the
  command needs one, as the reorder target.
- A workspace actor's key is `workspace_key` of the tab's identity: its local
  root, or `remote:<owner/repo>` for a remote PR. It never reads the project
  list, so a tab cannot drift to another actor.
- The inbox, the projects record, the PR cache and the GitHub status cache
  followed this home. They live in er-service's `app_records` module with
  the owner that publishes the sidebar's resources from them
  (`crates/er-service/src/app_records/app_owner.rs`). The desktop builds that
  owner, a second app-scoped owner beside `AppRegistry`. In its process it
  is the one writer of the projects record and the one saver of the inbox:
  the record's writers are private to the crate, and the desktop calls the
  owner's methods. Across processes, MCP's pin tools still write the record
  through the engine, and the store's scope lock orders the writers. The
  three cache kinds register through `register_desktop_kinds`, which only
  the desktop's composition calls, so the TUI and MCP registries leave them
  out.
