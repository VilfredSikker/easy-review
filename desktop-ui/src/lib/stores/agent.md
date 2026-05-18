# Stores Agent Guide

Stores coordinate frontend-only state around the backend snapshot. They should not become a second source of truth for review data.

## Store Map

- `app.svelte.ts`: owns `AppSnapshot`, polling, `app.cmd`, toasts, frontend logs, diff view mode, comment visibility, and coarse command loading flags.
- `browser.svelte.ts`: browser drawer state, current URL, annotation mode, and pending annotation interaction state.
- `browserUrl.ts`: canonical URL conversion between real URLs and the Tauri proxy schemes.
- `keyboard.ts`: global shortcut registration and command routing.
- `diffSelection.svelte.ts`: selected diff range and selected old/new side for comments/questions.
- `diffScroll.svelte.ts`: scroll positions and current file tracking for continuous diff.
- `terminal.svelte.ts`: terminal drawer/session frontend state.

## Rules

- `app.snapshot` is backend truth. Stores may cache UI preferences or in-progress interactions only.
- Keep polling simple. If the UI needs fresh data, fix backend revision invalidation before increasing poll frequency.
- `app.cmd` should remain the default mutation path because it standardizes snapshots, toasts, logs, and loading flags.
- Use direct `invoke` only for commands with non-snapshot return values, such as export preview or provider lists.
- LocalStorage keys should be stable and prefixed enough to avoid collisions.
- Keyboard handlers must avoid firing while textareas, inputs, terminal focus, or browser annotation modals are active.

## URL Canonicalization

Browser annotations depend on stable page identity. Use helpers in `browserUrl.ts` rather than hand-parsing URLs in components.

Current expectations:

- Real HTTP(S) URLs are proxied as `erp://` or `erps://` for iframe loading.
- `fromProxyUrl` returns the real URL for UI display and page matching.
- `sameBrowserUrl` prevents iframe reload feedback loops.
- Page-scoped annotation matching should use the canonical page key agreed with the backend, not raw user input.

## Loading And Errors

- Slow tab/branch/PR commands set `app.switching`.
- `force_refresh_diff` sets `app.refreshing`.
- Backend background activity renders from `snapshot.bg_loading` or `snapshot.background_tasks`.
- Errors should go through `pushLog` and `showToast`; avoid silent `catch` blocks except for expected polling/window-close noise.
