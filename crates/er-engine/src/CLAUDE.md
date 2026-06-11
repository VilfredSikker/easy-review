# er-engine/src — Source Overview

UI-agnostic core shared by the TUI (`crates/er-tui`) and the desktop app
(`crates/er-desktop`). No rendering and no event loop live here — those belong
to the consuming crates.

## Module Map

| Module | Purpose | Key file |
|--------|---------|----------|
| `app/` | All application state (`App`, `TabState`), navigation, comments, filters | `state/mod.rs` |
| `git/` | Diff parsing + git commands | `diff.rs`, `status.rs` |
| `ai/` | AI review data model, sidecar loader, prompts, comment storage | `review.rs`, `loader.rs` |
| `arena/` | Multi-reviewer "arena" runs (orchestrator + registry) | `orchestrator.rs` |
| `watch/` | Debounced file system watcher | `mod.rs` |
| `github.rs` | GitHub CLI (`gh`) integration: PRs, comment sync, status | — |
| `config.rs` | `ErConfig`, feature flags, settings items, TOML load/save | — |
| `storage.rs` | Managed review storage paths (repo/branch/view-bucket slugs) | — |
| `highlight.rs` | Syntect highlighter core (TUI wraps this; desktop uses Shiki) | — |
| `agent_slots.rs` | Process-wide counting semaphore for agent subprocess spawns | — |
| `dev_log.rs` | Opt-in debug log groups (`ER_LOG`) | — |

## Consumers

```
er-tui   main.rs event loop → input handlers → mutate App → ui::draw
er-desktop  Tauri commands  → mutate App      → build_snapshot → AppSnapshot
```

The TUI polls crossterm and the watch channel directly; the desktop wraps the
same `App` in a mutex behind Tauri commands and a poll/revision protocol. See
each crate's own docs (`crates/er-tui/src/ui/CLAUDE.md`,
`crates/er-desktop/agent.md`) for the surface-specific layers.

## github.rs

Parses GitHub PR URLs (`owner/repo/pull/N`) and shells out to `gh` — never the
HTTP API directly. Covers: PR metadata (`gh pr view`), read-only PR diffs,
checkout, base-branch resolution, open-PR detection for the current branch
(base hint), and two-way review comment sync (pull/push/reply/delete).
`REMOTE_PR_MAX_CHANGED_FILES` / `REMOTE_PR_MAX_LINE_CHANGES` guard pathological
remote PRs.
