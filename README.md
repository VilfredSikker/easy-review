# easy-review (`er`)

A git diff review tool built for developers who use AI coding assistants. Ships as a terminal TUI (`er`) and a desktop app (Tauri + Svelte) — both share the same review engine and data.

AI writes code faster than you can review it. `er` makes review fast, navigable, and live-updating.

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)

## Quick start

```bash
curl -fsSL https://raw.githubusercontent.com/VilfredSikker/easy-review/main/install.sh | bash
```

Installs a pre-built binary to `~/.local/bin/`. Then, from any git repository:

```bash
er
```

That's it — `er` diffs your current branch against an auto-detected base (main/master/develop) and opens the TUI. No config or setup required. Requires `git`; the optional `gh` CLI adds GitHub PR support.

### From source

```bash
git clone https://github.com/VilfredSikker/easy-review.git
cd easy-review
cargo install --path crates/er-tui
```

Requires Rust 1.85+.

## Desktop app

A graphical front end for the same review engine — split diffs, an embedded terminal and browser, a multi-model review arena, and point-and-click settings. It reads and writes the same review data as the terminal, so you can use both side by side.

As of **v0.4.0**, prebuilt Apple Silicon `.dmg` bundles are published on the [Releases page](https://github.com/VilfredSikker/easy-review/releases). Download it, open it, and drag **Easy Review** into Applications (right-click → **Open** the first time to bypass Gatekeeper, since the bundle isn't code-signed yet).

Intel Macs, Linux, and Windows aren't packaged yet — build from source instead:

```bash
git clone https://github.com/VilfredSikker/easy-review.git
cd easy-review
./scripts/tauri-dev.sh     # dev shell with hot reload
./scripts/tauri-build.sh   # release bundle
```

See [Installation](https://vilfredsikker.github.io/easy-review/guide/installation.html#desktop-app) for full details.

## MCP server (`er-mcp`)

A stdio [MCP](https://modelcontextprotocol.io) server for PR triage from Cursor/Claude — top priority PRs, low-hanging fruit, production-only diff line counts, outdated/blocked filters. See [`crates/er-mcp/README.md`](crates/er-mcp/README.md).

```bash
cargo install --path crates/er-mcp
```

## Documentation

📚 The full guide is at **[vilfredsikker.github.io/easy-review/guide](https://vilfredsikker.github.io/easy-review/guide/)** — [installation](https://vilfredsikker.github.io/easy-review/guide/installation.html), [quick start](https://vilfredsikker.github.io/easy-review/guide/quick-start.html), core concepts, keybindings, the AI review workflow, configuration, storage, GitHub sync, and troubleshooting for both the terminal UI and the desktop app.

## License

MIT
