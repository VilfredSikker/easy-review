# Engine App State Agent Guide

This directory owns the core review state used by both TUI and Desktop. Desktop-specific code should adapt this state, not fork the model unless the concern is truly desktop-only.

## Core Types

- `App`: top-level multi-tab state, overlays/hubs, AI provider selection, and app-level background tasks.
- `TabState`: one review target: working tree, local branch view, local PR ref, or remote PR cache.
- `DiffMode`: branch, unstaged, staged, history.
- `background.rs`: app-level background review task identity, lifecycle, and snapshots.
- `comments.rs`: comments, questions, findings, command spawning, AI review spawning, and background task polling.
- `github_sync.rs`: GitHub comment sync capture/fetch/apply flow.
- `navigation.rs`: file/hunk/line movement, compact/lazy parsing, scroll state, split-diff helpers.

## State Boundary

Use `TabState` for state that belongs to a specific review target:

- Diff files, selected file, hunk/line cursor, reviewed files.
- Local `.er`/managed comments directory via `er_root`.
- Per-tab command output for non-review AI actions.
- Local branch/PR identity and diff source refs.

Use `App` for state that must survive tab switches:

- Open tab list and active tab index.
- AI provider/model selection.
- Background review tasks that continue while the user navigates elsewhere.
- Recent completed background tasks for transient UI display.

Use `crates/er-desktop::AppState` instead for desktop-only caches:

- GitHub PR list cache.
- GitHub status cache.
- Loading flags, watcher status, terminal sessions, pending UI-only AI replies.

## Background Review Tasks

`background.rs` defines `BackgroundTaskTarget`, which deduplicates review tasks by repo, branch label, base, scope, PR number, and remote repo. `TabState::matches_target` is how task status is merged back into the active tab snapshot.

When changing this area:

- Keep task identity stable enough to prevent duplicate reviews for the same target.
- Do not make tasks restart-persistent unless explicitly requested; current scope is app-session only.
- Drain logs and result channels from the poll/tick path.
- Keep recently finished tasks only briefly so the UI can show done/failed toasts without unbounded growth.

## Comments And Threads

The engine stores questions and GitHub comments separately but exposes a unified thread shape to Desktop snapshots. Important conventions:

- Questions are private review notes.
- GitHub comments are publishable review comments and carry sync metadata.
- Replies are flat, not recursive.
- Findings are AI-produced but can be promoted to comments.
- Staleness is based on stored line content/diff context.

When adding comment-like entities, define the storage file, staleness behavior, sync behavior, and export behavior up front.

## Navigation And Diff Performance

`navigation.rs` is performance-sensitive. It handles lazy parsing, hunk offsets, current file/line, compacted files, context expansion, and split-side movement.

Avoid changes that rebuild all diff rows during simple cursor movement unless the Desktop snapshot contract requires it. For continuous diff UI, remember that the frontend may render many files while the engine cursor still identifies a single focused file.

## Backend Note

The engine is shared by TUI and Desktop. A shortcut that fixes Desktop by weakening `TabState` invariants can break the terminal app. Prefer adding explicit fields or adapters over making existing fields mean two different things.
