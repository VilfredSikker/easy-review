# Easy Review Agent Guide

This file is the top-level orientation for future coding agents. Existing `CLAUDE.md` files still describe the original TUI/core architecture; this `agent.md` layer focuses on the newer Desktop app work.

## Current System Shape

Easy Review now has three active surfaces:

- `crates/er-engine`: UI-agnostic review engine, git/diff state, comments, AI sidecar models, tabs, and session state.
- `crates/er-desktop`: Tauri backend bridge. Owns commands, snapshot wire types, desktop caches, background threads, browser proxy, tabs/projects persistence, terminal sessions, and export.
- `desktop-ui`: Svelte frontend. Consumes `AppSnapshot`, calls Tauri commands through `app.cmd`, and owns browser-only UI state such as diff rendering mode, keyboard routing, browser drawer state, and scroll/selection helpers.

The central contract is snapshot-on-command plus polling:

1. Frontend calls a Tauri command.
2. Backend mutates `App` or desktop-owned caches.
3. Backend returns a full `AppSnapshot`.
4. Frontend polling calls `poll`, which returns a full snapshot only when the computed revision changes.

## Broad Improvement Patterns

- Keep state ownership explicit. Engine state belongs in `App`/`TabState`; desktop-only cache and background state belongs in `AppState`; frontend ephemeral UI state belongs in Svelte stores.
- Avoid holding the app mutex during network or subprocess work. Capture the minimum context under lock, release it, run the slow operation, then apply the result and bump `desktop_revision`.
- Any desktop-owned background mutation must invalidate polling. If a cache, loading flag, watcher status, or async task changes outside `App`, bump `desktop_revision` or the UI may not refresh.
- Do not route new features through transient frontend-only logs. User-visible failures should produce durable Rust `log::error!` entries with repo, tab, branch/PR, command, and stderr/status context.
- Treat `AppSnapshot` and `desktop-ui/src/lib/types.ts` as one wire contract. Add fields in both places and keep optional/default handling stable for older snapshots.
- Prefer read-only PR review. Do not use checkout-based flows for PR inspection unless the user explicitly wants to mutate the worktree.
- Keep performance work close to hot paths. Snapshot construction, syntax highlighting, JSON serialization, browser proxy response size, and Svelte DOM row counts are the likely freeze amplifiers.
- Preserve the user's current worktree. This repo often has large uncommitted Desktop changes; inspect before editing and never revert unrelated files.

## Feature Map

- Multi-tab review: `crates/er-engine/src/app/state/mod.rs`, `crates/er-desktop/src/tabs.rs`, `desktop-ui/src/lib/components/TabStrip.svelte`.
- Projects/sidebar PR lists: `crates/er-desktop/src/projects.rs`, `crates/er-desktop/src/pr_cache.rs`, `crates/er-desktop/src/snapshot.rs`, `desktop-ui/src/lib/components/LeftSidebar.svelte`.
- Background AI review tasks: `crates/er-engine/src/app/state/background.rs`, `crates/er-engine/src/app/state/comments.rs`, `desktop-ui/src/lib/components/BackgroundTasks.svelte`, `desktop-ui/src/lib/components/AgentOutputCard.svelte`.
- GitHub status/review submission: `crates/er-desktop/src/commands.rs`, `crates/er-engine/src/github.rs`, `crates/er-engine/src/app/state/github_sync.rs`, `desktop-ui/src/lib/components/BranchCard.svelte`, `desktop-ui/src/lib/components/CommentsCard.svelte`.
- Browser annotations: `crates/er-desktop/src/main.rs`, `crates/er-desktop/src/commands.rs`, `crates/er-engine/src/ai/comments.rs`, `desktop-ui/src/lib/components/BrowserView.svelte`, `desktop-ui/src/lib/components/AnnotationOverlay.svelte`, `desktop-ui/src/lib/stores/browserUrl.ts`.
- Diff rendering: `crates/er-desktop/src/snapshot.rs`, `crates/er-engine/src/app/state/navigation.rs`, `desktop-ui/src/lib/components/DiffView.svelte`, `desktop-ui/src/lib/splitRows.ts`, `desktop-ui/src/lib/stores/diffSelection.svelte.ts`, `desktop-ui/src/lib/stores/diffScroll.svelte.ts`.
- Export: `crates/er-desktop/src/export.rs`, `crates/er-desktop/src/commands.rs`, `desktop-ui/src/lib/components/ExportModal.svelte`.
- Terminal drawer: `crates/er-desktop/src/terminal.rs`, terminal commands in `crates/er-desktop/src/commands.rs`, `desktop-ui/src/lib/components/Terminal.svelte`, `desktop-ui/src/lib/stores/terminal.svelte.ts`.
- Desktop-managed review storage: `crates/er-desktop/src/er_storage.rs`, `crates/er-desktop/src/tabs.rs`, and `TabState::er_root`.

## Investigation Order For Future Work

1. Start from the user-visible symptom and identify the feature area from the map above.
2. Inspect the Svelte component and store that initiates the command.
3. Inspect the matching Tauri command in `crates/er-desktop/src/commands.rs`.
4. Inspect whether the command mutates engine state, desktop cache state, or files under `.er`/managed storage.
5. Inspect `build_snapshot` to confirm the changed state actually reaches the frontend.
6. Check polling invalidation: `compute_poll_revision`, `desktop_revision`, and any snapshot hash inputs.
7. Add or update tests at the lowest layer that owns the behavior, then run the narrowest relevant check.

## Verification Shortlist

- Rust command and state tests: `cargo test -p er-desktop` and targeted `cargo test -p er-engine <filter>`.
- Frontend utility/component tests: run from `desktop-ui` with the package's test script if present.
- Snapshot contract changes: inspect both Rust `AppSnapshot` and TypeScript `AppSnapshot`.
- Browser annotation changes: verify URL canonicalization, proxy navigation, selector re-anchoring, stale behavior, and annotation persistence.
- GitHub/PR changes: verify behavior without mutating the current checkout unless mutation is the explicit goal.
