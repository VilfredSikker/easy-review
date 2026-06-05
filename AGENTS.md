# AGENTS.md

## Cursor Cloud specific instructions

This is a single-crate Rust CLI tool (`er`) with no external services. See `CLAUDE.md` for full architecture and code conventions.

### Build / Test / Lint / Run

| Task | Command |
|------|---------|
| Build (dev) | `cargo build` |
| Build (release) | `cargo build --release` |
| Install binary | `cargo install --path .` |
| Run | `er` (from any git repo) |
| Test | `cargo test` |
| Clippy | `cargo clippy --all-targets -- -D warnings` |
| Format check | `cargo fmt --all -- --check` |
| Debug mode | `ER_DEBUG=1 er` (logs to `/tmp/er_debug.log`) |

### Gotchas

- **Rust toolchain**: easy-review needs Rust **1.85+** (`edition2024`). Cloud VMs may ship an older or already-current stable under `/usr/local/rustup`. The install hook [`scripts/cloud-agent-install.sh`](scripts/cloud-agent-install.sh) runs `rustup update stable` **only when** `rustc` is missing or < 1.85 — unconditional `rustup update` fails on overlayfs (EXDEV / “Invalid cross-device link”) when updating the baked system toolchain. It always runs `rustup default stable` and `cargo fetch`.
- **TUI requires a terminal**: `er` renders via crossterm/ratatui, so it must run inside a real terminal (tmux session, not a headless pipe). Use `tmux` to launch and `computerUse` subagent to interact with it.
- **Clippy warnings**: As of the current codebase, `cargo clippy --all-targets -- -D warnings` reports 9 pre-existing warnings (collapsible_match, unnecessary_sort_by, manual_checked_ops). These are not regressions — they exist on `main`.
- **No external services**: No databases, Docker, or network services needed. The only runtime dependency is `git` (and optionally `gh` CLI for GitHub PR features).
- **`.er/` directory**: AI sidecar files are read from `.er/` in the repo root. This directory is gitignored. The TUI creates `.er/session.json` on first run.
