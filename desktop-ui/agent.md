# Desktop UI Agent Guide

`desktop-ui` is a Svelte frontend for the Tauri Desktop app. It should be treated as a consumer of the Rust snapshot contract plus a small owner of ephemeral browser/UI state.

## Main Entry Points

- `src/App.svelte`: app shell, title bar, panels, tab strip, terminal/browser drawers, global overlays.
- `src/lib/types.ts`: TypeScript mirror of `crates/er-desktop/src/snapshot.rs`.
- `src/lib/stores/app.svelte.ts`: snapshot store, polling, Tauri command wrapper, toasts, logs, diff mode, comment visibility.
- `src/lib/stores/keyboard.ts`: global keyboard routing.
- `src/lib/stores/browser.svelte.ts` and `browserUrl.ts`: browser drawer state and proxy URL canonicalization.
- `src/lib/components`: feature components.
- `src/lib/components/ui`: small shared UI primitives.
- `src/lib/stories`: Storybook scenarios and visual fixtures.

## Frontend State Rules

- `app.snapshot` is backend truth. Do not duplicate persisted review state in frontend stores.
- LocalStorage is acceptable for frontend preferences such as diff view mode, comment visibility, drawer height, and scroll positions.
- Always call backend mutations through `app.cmd` unless a command needs a custom return type, such as export preview.
- If an action can fail, use `app.cmd` so errors reach toasts and logs consistently.
- Do not infer backend state from toasts. Render from snapshot fields.

## Polling Model

`app.startPolling` calls `poll` every 2s and updates the snapshot only when the revision changes. If the UI looks stale after a backend mutation, inspect backend invalidation before adding frontend timers.

## UI Performance Risks

- `DiffView.svelte` can render many files and many rows. Use windowed rendering, stable keys, measured placeholder heights, and avoid per-scroll backend calls.
- Avoid repeatedly serializing or deep comparing full snapshots in the frontend.
- Keep expensive DOM work behind observers, requestAnimationFrame, or coarse timers.
- Browser annotations post messages frequently. Do not persist every hover; persist only committed annotations or re-anchor results.

## Visual/Interaction Conventions

- Preserve the established dark, dense review UI unless a task explicitly asks for a redesign.
- All colors must flow through the design tokens in `src/app.css` (`@theme` block), which `src/lib/themes.ts` overrides per theme. Use the token-backed Tailwind utilities (`text-fg-3`, `bg-card`, `text-error`, `bg-success/10`, …) or `var(--color-*)` in inline styles — never raw hex/rgb values and never stock Tailwind palette classes (`text-amber-400`, `bg-black`, `text-white`), which resolve to Tailwind defaults and ignore the theme. For alpha tints use `color-mix(in srgb, var(--color-x) N%, transparent)`. SVG `stroke`/`fill` attributes can't hold `var()` — use `stroke="currentColor"` plus a text-* class, or a `style` attribute. Exceptions: the arena overlay's fixed `--arena-*` palette (self-contained by design, including agent identity colors in `arena/agents.ts`), `spanColorRemap.ts` (Shiki token-color transformation), and Storybook harnesses.
- Prefer small shared primitives from `components/ui` over ad hoc styling for repeated card/button/pill patterns.
- Keyboard shortcuts should be registered centrally in `keyboard.ts`, while component-local text inputs must stop propagation where needed.
- Any command palette action should map to an existing Tauri command or a clearly documented frontend-only preference.

## Contract Checklist

Before shipping a UI feature:

1. Confirm the needed field exists in `types.ts` and `snapshot.rs`.
2. Confirm the component renders from snapshot data, not stale local copies.
3. Confirm command names and argument casing match Rust Tauri commands.
4. Confirm loading/error states use `bg_loading`, `app.switching`, `app.refreshing`, toasts, or explicit component state.
5. Add or update Storybook stories when the feature changes a major layout state.
