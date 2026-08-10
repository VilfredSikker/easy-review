# Easy Review Herdr plugin

[Herdr](https://herdr.dev) plugin that opens the `er` TUI in a workspace tab, auto-opens review when a new worktree is created, and handles Ctrl-click on GitHub PR URLs.

Requires [Herdr](https://herdr.dev) **0.7.0+** and the `er` CLI on `PATH`.

## Install

From GitHub (after this ships):

```bash
herdr plugin install VilfredSikker/easy-review/tools/herdr-easy-review --yes
```

While developing in this repository:

```bash
herdr plugin link /path/to/easy-review/tools/herdr-easy-review
herdr plugin action list --plugin easy-review
```

Install `er` if you have not already:

```bash
cargo tui-install   # from an easy-review clone
# or: curl -fsSL https://raw.githubusercontent.com/VilfredSikker/easy-review/main/install.sh | bash
```

## What it does

| Entry | Trigger | Behavior |
|-------|---------|----------|
| **Review** pane | `herdr plugin pane open --plugin easy-review --entrypoint review` | Runs `er` in the workspace cwd (reviews the checked-out branch). |
| **worktree.created** event | New worktree | Opens a Review tab for the new workspace (`--no-focus`). |
| **review-branch** action | Workspace context menu / keybinding | Opens Review tab for the current workspace (`--focus`). |
| **review-pr** action / link handler | Ctrl-click a `github.com/.../pull/N` URL | Opens Review tab, then `er --remote <url>`. |

## Optional keybinding

Bind a shortcut to review the current branch:

```toml
# ~/.config/herdr/config.toml
[[keys.command]]
key = "prefix+r"
type = "plugin_action"
command = "easy-review.review-branch"
description = "Review branch in Easy Review"
```

## Test

```bash
cd tools/herdr-easy-review
npm test
```

From the repo root: `just test-herdr-plugin`.

## Docs

User guide: [Herdr integration](https://vilfredsikker.github.io/easy-review/guide/herdr.html) (in-repo: [`docs/guide/herdr.html`](../../docs/guide/herdr.html)).

## Trust

This plugin runs shell scripts on your machine with your user permissions. They only call `herdr` and `er` — review `herdr-plugin.toml` and the `*.sh` files before installing.
