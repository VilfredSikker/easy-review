# er-github — the GitHub port

Everything that spawns `gh` lives here. Callers hold a `GitHub` handle instead of
a free function, so a test can answer with `FakeGitHub`.
`docs/adr/0055-gh-errors-are-classified-by-http-status-then-stderr.md`.

## Boundaries

- GitHub access shells out to `gh`, never the HTTP API (ADR 0026). A future HTTP
  client becomes a second `GitHub` implementation.
- Errors are classified only in `classify.rs`. No other module reads `gh`
  stderr: a caller matches `GhError`, never text.
- A new `gh` command is a new `GitHub` trait method, its `CliGitHub` method, its
  `FakeGitHub` slot and a `ScriptedRunner` test, all in the PR that needs it.
  Never spawn `gh` from a caller: the `er::gh_spawn` gate fails the build.
- The caches live on `CliGitHub` (in `cli.rs`), one `TtlCache` per answer, each
  with its own generation counter. A failure is never stored.

## The handle

The convention follows `crates/er-git/CLAUDE.md`: one trait object passed in,
and no global that tests reach. Production wrappers use `default_handle()`;
tests build `CliGitHub` on a `ScriptedRunner`. The `github` field on
`er_engine::ports::Ports` carries the handle.

## Layout

The moved `github.rs` bodies are grouped by concern: checkout, diff, commits,
comments, overview, repo and lookup. Put a new function beside the one that
calls it most; the tree is the inventory.

## Compatibility wrappers

The front ends and the engine still call the old free-function names. Each is
a one-expression wrapper over `default_handle()` with its old
`anyhow::Result` (or `Option`) return. Where the old code wrapped gh's text in
a sentence, the wrapper uses `anyhow!("{old}: {e}")`; where it had none, the
error passes through. They go when the callers take the handle.
