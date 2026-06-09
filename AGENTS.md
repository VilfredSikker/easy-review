# AGENTS.md

## Cursor Cloud specific instructions

This is a single-crate Rust CLI tool (`er`) with no external services. See `CLAUDE.md` for full architecture and code conventions.

### Build / Test / Lint / Run

| Task | Command |
|------|---------|
| Build TUI (dev) | `./scripts/er-tui.sh build -p er-tui` or `cargo tui-build` (needs `.cargo/bin` on `PATH`; see `.envrc`) |
| Build TUI (release) | `./scripts/er-tui.sh build --release -p er-tui` or `cargo tui-release` |
| Install binary | `cargo tui-install` or `cargo install --path crates/er-tui` |
| Run TUI | `er` (from any git repo) or `cargo tui-run` |
| Test TUI/engine | `./scripts/er-tui.sh test -p er-engine -p er-tui` or `cargo tui-test` |
| Desktop release | `./scripts/tauri-build.sh` or `cargo desktop-release` |
| Test full workspace | `cargo test --workspace` (slow — builds Tauri) |
| Desktop dev | `./scripts/tauri-dev.sh` |
| Reclaim `target/` disk | `./scripts/cargo-gc.sh` (also runs from dev scripts) |
| Clippy | `cargo clippy --all-targets -- -D warnings` |
| Format check | `cargo fmt --all -- --check` |
| Debug mode | `ER_DEBUG=1 er` (overwrites `/tmp/er_debug.log` each git diff) |

### Gotchas

- **`target/` bloat**: `cargo test` / `cargo build` without `-p` compile **er-desktop** (Tauri) into shared `target/`, which can grow to tens of GB (300k+ stale `debug/deps` files). Use `./scripts/er-tui.sh` for TUI work (`target/tui`), `./scripts/tauri-dev.sh` for desktop (`target/desktop`). Run `./scripts/cargo-gc.sh` to prune legacy `target/debug` when over ~8 GB.
- **Rust toolchain**: easy-review needs Rust **1.85+** (`edition2024`). Cloud VMs may ship an older or already-current stable under `/usr/local/rustup`. The install hook [`scripts/cloud-agent-install.sh`](scripts/cloud-agent-install.sh) runs `rustup update stable` **only when** `rustc` is missing or < 1.85 — unconditional `rustup update` fails on overlayfs (EXDEV / “Invalid cross-device link”) when updating the baked system toolchain. It always runs `rustup default stable` and `cargo fetch`.
- **TUI requires a terminal**: `er` renders via crossterm/ratatui, so it must run inside a real terminal (tmux session, not a headless pipe). Use `tmux` to launch and `computerUse` subagent to interact with it.
- **Clippy warnings**: As of the current codebase, `cargo clippy --all-targets -- -D warnings` reports 9 pre-existing warnings (collapsible_match, unnecessary_sort_by, manual_checked_ops). These are not regressions — they exist on `main`.
- **No external services**: No databases, Docker, or network services needed. The only runtime dependency is `git` (and optionally `gh` CLI for GitHub PR features).
- **`.er/` directory**: AI sidecar files are read from `.er/` in the repo root. This directory is gitignored. The TUI creates `.er/session.json` on first run.
