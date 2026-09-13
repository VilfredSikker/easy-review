# AGENTS.md

Pointer index for coding agents other than Claude Code. Codex, Cursor, and Gemini
discover this filename and read nothing else, so it holds only what has no other
home.

- [`CLAUDE.md`](CLAUDE.md) — architecture, module and layer boundaries, conventions, traps.
- [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) — build, test, lint, dev, and release mechanics.
- [`docs/adr/`](docs/adr/) — numbered decisions and why they were made. Read the ones covering your area, and cite the number rather than restating the decision.
- [`crates/er-desktop/agent.md`](crates/er-desktop/agent.md), [`desktop-ui/agent.md`](desktop-ui/agent.md) — desktop backend and frontend notes.
- [`CONTEXT.md`](CONTEXT.md), [`docs/agents/`](docs/agents/) — project vocabulary, issue tracking, triage labels.

## Build / Test / Lint / Run

[`just`](https://just.systems) is the front-end: bare `just` lists recipes, and
`just run`, `just test`, `just install`, `just lint`, `just ci` cover the common
ones. It delegates to the wrapper scripts in `scripts/`, which own the split
target dirs. Cargo aliases, per-crate dev loops, desktop dev, and signing live in
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md).

## Traps

- **The TUI needs a real terminal.** `er` renders through crossterm/ratatui, so a
  headless or piped invocation fails. Run it in a pty — a tmux session is the
  usual answer.
- **`ER_DEBUG=1 er` rewrites `/tmp/er_debug.log` on every git diff call.** The
  file holds only the most recent call's command, exit code, and stderr, so
  copy it out before the next refresh. (`crates/er-engine/src/git/status.rs`)

## GitHub repo metadata

Topics, description, and homepage live on GitHub, not in git.
`VilfredSikker/easy-review` must keep the `herdr-plugin` topic (Herdr
marketplace) plus product topics for search:

`ai-code-review`, `cli`, `code-review`, `desktop-app`, `developer-tools`,
`diff`, `git`, `github`, `herdr`, `mcp`, `model-context-protocol`,
`pull-requests`, `ratatui`, `rust`, `svelte`, `tauri`, `tui`.

Set with `gh repo edit --add-topic …`. Description is the one-line GitHub blurb;
homepage points at the docs site.
