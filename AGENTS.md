# AGENTS.md

Orientation for coding agents working in this repo. `CLAUDE.md` is the full
architecture and conventions reference; this file covers build/test commands,
environment gotchas, and a map of the desktop app, which spans three layers.

## Build / Test / Lint / Run

A [`just`](https://just.systems) front-end wraps these (`just` to list, e.g.
`just run`, `just test`, `just lint`, `just ci`). It delegates to the same
scripts/aliases below, so either form works.

| Task | Command |
|------|---------|
| Build TUI (dev) | `./scripts/er-tui.sh build -p er-tui` or `cargo tui-build` (needs `.cargo/bin` on `PATH`; see `.envrc`) |
| Build TUI (release) | `./scripts/er-tui.sh build --release -p er-tui` or `cargo tui-release` |
| Install binary | `cargo tui-install` or `cargo install --path crates/er-tui` |
| Run TUI | `er` (from any git repo) or `cargo tui-run` |
| Test TUI/engine | `./scripts/er-tui.sh test -p er-engine -p er-tui` or `cargo tui-test` |
| Test desktop backend | `cargo test -p er-desktop` |
| Build Easy Review MCP | `cargo build -p er-mcp` (stdio server; setup: `docs/guide/mcp.html`) |
| npm MCP launcher | `npm/er-mcp` — `npx -y easy-review-mcp` (downloads release binary) |
| Install ER agent skill | `npx skills add VilfredSikker/easy-review -s er-review -g` — see `skills/er-review/` |
| Desktop dev | `./scripts/tauri-dev.sh` |
| Desktop release (local/ad-hoc) | `./scripts/tauri-build.sh` or `cargo desktop-release` |
| Desktop signed release | `just sign` (or `just sign-release-desktop`) — guide: [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md#macos-signed-release-developer-id--notarization) (`.env.signing`) |
| Frontend checks | `cd desktop-ui && bun run check && bun test src` |
| Test full workspace | `cargo test --workspace` (slow — builds Tauri) |
| Reclaim `target/` disk | `./scripts/cargo-gc.sh` (also runs from dev scripts) |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` |
| Format check | `cargo fmt --all -- --check` |
| Debug mode | `ER_DEBUG=1 er` (overwrites `/tmp/er_debug.log` each git diff) |

## Environment Gotchas

- **`target/` bloat**: `cargo test` / `cargo build` without `-p` compile **er-desktop** (Tauri) into shared `target/`, which can grow to tens of GB. Use `./scripts/er-tui.sh` for TUI work (`target/tui`), `./scripts/tauri-dev.sh` for desktop (`target/desktop`). Run `./scripts/cargo-gc.sh` to prune.
- **Rust toolchain**: needs Rust **1.85+** (`edition2024`). The cloud install hook [`scripts/cloud-agent-install.sh`](scripts/cloud-agent-install.sh) runs `rustup update stable` **only when** `rustc` is missing or < 1.85 — unconditional `rustup update` fails on overlayfs (EXDEV) when updating a baked system toolchain.
- **TUI requires a terminal**: `er` renders via crossterm/ratatui, so it must run inside a real terminal (e.g. a tmux session), not a headless pipe.
- **No external services**: no databases, Docker, or network services. The only runtime dependency is `git` (and optionally `gh` for GitHub PR features).
- **Review sidecars**: AI sidecar files live in managed app data by default (see "Managed review storage" in `CLAUDE.md`). Set `ER_REPO_LOCAL=1` to use repo-local `.er/` instead.

## Desktop App Shape

Three active surfaces:

- `crates/er-engine`: UI-agnostic review engine — git/diff state, comments, AI sidecar models, tabs, session state.
- `crates/er-desktop`: Tauri backend bridge — commands, snapshot wire types, desktop caches, background threads, browser proxy, tabs/projects persistence, terminal sessions, export. See [`crates/er-desktop/agent.md`](crates/er-desktop/agent.md).
- `desktop-ui`: Svelte frontend — consumes `AppSnapshot`, calls Tauri commands through `app.cmd`, owns browser-only UI state (diff rendering mode, keyboard routing, drawer state, scroll/selection).

The central contract is snapshot-on-command plus polling:

1. Frontend calls a Tauri command.
2. Backend mutates `App` or desktop-owned caches.
3. Backend returns a full `AppSnapshot`.
4. Frontend polling calls `poll`, which returns a full snapshot only when the computed revision changes.

## Desktop Working Rules

- Keep state ownership explicit: engine state in `App`/`TabState`; desktop-only cache and background state in `AppState`; frontend ephemeral UI state in Svelte stores.
- Never hold the app mutex during network or subprocess work. Capture the minimum context under lock, release it, run the slow operation, then apply the result and bump `desktop_revision`.
- Any desktop-owned background mutation must invalidate polling — bump `desktop_revision` or the UI won't refresh.
- User-visible failures should produce durable Rust `log::error!` entries with repo, tab, branch/PR, command, and stderr/status context — not transient frontend-only logs.
- Treat Rust `AppSnapshot` and `desktop-ui/src/lib/types.ts` as one wire contract: add fields in both places and keep optional/default handling stable.
- Prefer read-only PR review; don't use checkout-based flows unless the user explicitly wants to mutate the worktree.
- Hot paths that amplify freezes: snapshot construction, syntax highlighting, JSON serialization, browser proxy response size, Svelte DOM row counts.

## Feature Map

- Multi-tab review: `crates/er-engine/src/app/state/mod.rs`, `crates/er-desktop/src/tabs.rs`, `desktop-ui/src/lib/components/TabStrip.svelte`.
- Projects/sidebar PR lists: `crates/er-desktop/src/projects.rs`, `crates/er-desktop/src/pr_cache.rs`, `crates/er-desktop/src/snapshot.rs`, `desktop-ui/src/lib/components/LeftSidebar.svelte`.
- Background AI review tasks: `crates/er-engine/src/app/state/background.rs`, `crates/er-engine/src/app/state/comments.rs`, `desktop-ui/src/lib/components/BackgroundTasks.svelte`, `desktop-ui/src/lib/components/AgentOutputCard.svelte`.
- GitHub status/review submission: `crates/er-desktop/src/commands.rs`, `crates/er-engine/src/github.rs`, `crates/er-engine/src/app/state/github_sync.rs`, `desktop-ui/src/lib/components/BranchCard.svelte`, `desktop-ui/src/lib/components/CommentsCard.svelte`.
- Browser annotations: `crates/er-desktop/src/main.rs`, `crates/er-desktop/src/commands.rs`, `crates/er-engine/src/ai/comments.rs`, `desktop-ui/src/lib/components/BrowserView.svelte`, `desktop-ui/src/lib/components/AnnotationOverlay.svelte`, `desktop-ui/src/lib/stores/browserUrl.ts`.
- Diff rendering: `crates/er-desktop/src/snapshot.rs`, `crates/er-engine/src/app/state/navigation.rs`, `desktop-ui/src/lib/components/DiffView.svelte`, `desktop-ui/src/lib/splitRows.ts`, `desktop-ui/src/lib/stores/diffSelection.svelte.ts`, `desktop-ui/src/lib/stores/diffScroll.svelte.ts`.
- Export: `crates/er-desktop/src/export.rs`, `desktop-ui/src/lib/components/ExportModal.svelte`.
- Terminal drawer: `crates/er-desktop/src/terminal.rs`, `desktop-ui/src/lib/components/Terminal.svelte`, `desktop-ui/src/lib/stores/terminal.svelte.ts`.
- Desktop-managed review storage: `crates/er-desktop/src/er_storage.rs`, `crates/er-desktop/src/tabs.rs`, `TabState::er_root`.

## Investigating Desktop Issues

1. Identify the feature area from the map above.
2. Inspect the Svelte component and store that initiates the command.
3. Inspect the matching Tauri command in `crates/er-desktop/src/commands.rs`.
4. Determine whether the command mutates engine state, desktop cache state, or files under managed storage.
5. Confirm in `build_snapshot` that the changed state actually reaches the frontend.
6. Check polling invalidation: `compute_poll_revision`, `desktop_revision`, and snapshot hash inputs.
7. Add or update tests at the lowest layer that owns the behavior, then run the narrowest relevant check.
