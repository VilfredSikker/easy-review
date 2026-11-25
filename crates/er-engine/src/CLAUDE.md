# er-engine — core boundaries

UI-agnostic core shared by `er-tui`, `er-desktop`, and `er-mcp`. Nothing here
renders and nothing here runs an event loop; the crate has no async items and no
runtime handle. Each front end owns its own loop, and only the front ends that
need tokio declare it. See `docs/adr/0002-synchronous-engine-and-tui.md`.

## Where the line is

The engine holds state and the operations on it, not the surface that shows it.
Anything a second front end could reuse belongs here; anything that exists only
because one surface draws or keys it belongs there.

Both front ends read the tab content the service publishes as shared sections
(ADR 0071), and view state with one front-end owner lives in that front end
(ADR 0068). When a front end needs something a section does not express, add an
explicit field or translate in that front end. Never widen or repurpose an
existing field so it carries a second meaning: it compiles for both front ends,
so nothing flags it, and it breaks at runtime in whichever one the change was
not written for.

## Feature gating

Always-on modules must not depend on a feature-gated one; a headless build breaks
at compile time when one reaches into the other. Which module sits on which side
is `#[cfg(feature = ...)]` in `lib.rs`, matched against `[features]` in
`Cargo.toml`. Headless consumers build with `default-features = false`.

## Rules that outlive a refactor

- GitHub access shells out to the `gh` CLI. No HTTP client, no token, in config
  or in code. `docs/adr/0026-gh-cli-not-http-api.md`.
- Agent subprocess spawns are capped by a FIFO queue plus the process-wide
  `agent_slots` semaphore. Only the paths that acquire a slot are capped; a new
  spawn path is uncapped unless it acquires one. `docs/adr/0021-agent-concurrency.md`.

Submodule detail lives next to the code: `ai/CLAUDE.md`, `git/CLAUDE.md`,
`watch/CLAUDE.md`.
