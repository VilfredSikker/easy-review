# The TUI reads the service in-process

## Context

Phase 5b moved the TUI off the App and onto the Workspace actors (ADR 0067).
The desktop reaches the actors through versioned resources over IPC (ADR
0072), and its tab list is the app registry that saves `tabs.json` and
`pr-open-cache.json` (ADR 0070). The TUI runs the service in its own process,
opens the tabs its command line names, and must redraw a frame without
waiting on anything.

## Decision

The TUI holds one `ServiceHost`
(`crates/er-service/src/host.rs`): an actor per workspace key, the settings
actor and one app registry, each started on first use.

- **The read path is in-process.** Each frame reads the active tab's
  published `TabContent` and draws its sections with the engine types they
  hold. A value phase 5 computes on the server (reviewed counts, thread
  ordering, the findings projection, job rows) comes from the same
  er-service builder the desktop's resources call, once per section
  revision, never once per frame. Nothing is serialized, and the TUI writes
  no rule a builder already holds.
- **The registry lives in memory.** It issues every key and lists every tab,
  so the actors refuse a load for a key it does not list, but it never reads
  or writes `tabs.json` or `pr-open-cache.json`. The TUI's tabs come from its
  arguments, and a TUI must never rewrite the tabs the desktop saved.
- **View state is the TUI's own.** `TuiView` holds, per tab key, the
  selection, cursor, scroll, panel, layers, filter, layout widths and text
  areas, and app wide the input mode and the overlay. Navigation runs on it
  through `er_engine::navigation`. `session.json` is its file: saved from the
  `TuiView` beside the tab's branch diff hash, restored only on the same hash.
- **Nothing on the loop waits on an actor.** Commands go out with `post`,
  replies are drained each tick, and an event from any actor or the registry
  asks for a redraw. Each second the loop asks every loaded tab to read its
  sidecars again, so a write made outside the process reaches the frame.

## Considered Options

- **Compute the derived values in er-tui from the sections.** Two front ends
  would hold one rule each, and drift between them is the defect the review
  rules rank highest.
- **Render the desktop's resources.** The TUI would decode a wire format it
  produced in the same process, for no reader outside it.
- **Share the desktop's saved registry.** The TUI would open the desktop's
  tabs, and the last of the two to save would overwrite the other's.

## Consequences

- er-engine has had no App and no `ui` feature since P77; er-engine's
  manifest test keeps the feature from coming back. er-tui's manifest test
  holds its er-engine lines to `default-features = false`, and the own-graph
  check (`cargo check -p er-tui --all-targets`) fails if a module relies on
  an engine feature that only another crate turns on.
- A source scan walks `crates/er-tui/src` and keeps every module from naming
  the App, from calling `send_and_wait` or `recv_timeout`, from writing or
  discovering settings itself, and from starting a thread. Three threads are
  allowed, each a reader: the watched-file reader (`service/watched.rs`), which
  the scan holds to er-git's read functions; the PR probe
  (`service/pr_probe.rs`), whose calls `github_tests` checks; and the
  background update check `main.rs` starts from `update.rs`, whose result the
  loop reads without waiting. A test holds a load on a gated git port and
  bounds each key's frame.
- A write guard (`crates/er-tui/tests/writes.rs`) keeps every module from
  writing a store artifact, the working tree or GitHub itself, through the
  ports, er-github, er-store, er-engine's writers or `std::fs`. It exempts
  `service/session.rs`'s save and `update.rs`'s archive and binary. One form
  is out of its reach: a git push whose result a match arm or a closure
  returns, which reads the same as a list's push. The same walk fails on any path that says an area is
  not on this build, so no key, hub row or panel falls back to one.
- The TUI and the desktop each run a settings actor over one `config.toml`,
  as the two processes did before; ADR 0005's rule against a reload that
  overwrites live config still holds.
