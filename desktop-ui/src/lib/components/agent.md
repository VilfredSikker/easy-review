# Components Agent Guide

This directory contains Desktop feature components. Most components are thin views over `app.snapshot` plus calls to `app.cmd`.

## Feature Ownership

- `TabStrip.svelte`: open tab list, select/close/reorder tabs, panel toggles (left/tree/right). Backend commands: `new_tab`, `select_tab`, `close_tab`, `reorder_tabs`.
- `BranchContextBar.svelte`: active branch chip, base label, copy branch/path actions (row below tab strip).
- `LeftSidebar.svelte`: projects, tracked branches, PR buckets, refresh/dismiss/track actions.
- `BranchCard.svelte`: active branch/PR summary, GitHub status, checks/reviews, watcher state, GitHub refresh.
- `FileTree.svelte`: file index, reviewed toggles, continuous diff jump-to-file behavior.
- `DiffView.svelte`: continuous diff rendering, split/unified modes, windowed file bodies, inline findings/threads, diff selection and composers.
- `InlineThread.svelte`, `InlineFinding.svelte`, `DiffComposer.svelte`, `PromoteModal.svelte`: comments, questions, replies, finding promotion, and line selection composition.
- `AiReviewCard.svelte`, `AiActionPalette.svelte`, `AgentOutputCard.svelte`, `BackgroundTasks.svelte`: AI review controls, active model actions, per-tab logs, app-level background tasks.
- `BrowserView.svelte`, `AnnotationOverlay.svelte`, `UiAnnotationsCard.svelte`: embedded browser, DOM annotation capture, re-anchor, visibility, and annotation list actions.
- `CommandPalette.svelte`: global command discovery and command execution.
- `ExportModal.svelte`: Markdown export preview/copy/write flow.
- `Terminal.svelte`: PTY drawer UI.
- `Toast.svelte`, `BottomHints.svelte`, `EmptyState.svelte`: global shell feedback.

## Component Rules

- Prefer props for Storybook/test fixtures and `app.snapshot` for live app data.
- Keep backend command names centralized in component actions; do not build dynamic command strings from user input.
- Use `app.cmd` for commands returning `AppSnapshot`; use `invoke` directly only when the command returns a different type.
- For destructive or bulk actions, confirm in the component and then call the backend command.
- Keep per-feature loading local only when the backend has no global loading flag. Otherwise render from `snapshot.bg_loading`.

## High-Risk Components

- `DiffView.svelte`: preserve scroll position and placeholder heights when changing windowed rendering.
- `BrowserView.svelte`: avoid iframe reload loops. Compare canonical real URLs with `sameBrowserUrl` before changing `iframeSrc`.
- `AnnotationOverlay.svelte`: annotation visibility should be scoped to canonical page key, not a global list.
- `CommentsCard.svelte`: review submission should not mark comments synced optimistically; backend owns sync state.
- `LeftSidebar.svelte`: PR list refresh should stay on-demand/deduped, not aggressive polling.

## Storybook

When adding a substantial UI state, add a story under `src/lib/stories` using existing fixtures. Good story candidates are large diffs, sparse diffs, PR GitHub status, annotation mode, background tasks, empty state, and export modal.
