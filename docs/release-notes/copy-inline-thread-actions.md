# Copy works on inline comments, findings, notes, and questions

## What changed

The **Copy** action in an inline thread's action bar (desktop) now actually
copies. It was silently doing nothing on all four inline-thread surfaces —
comments, findings, notes, and questions — which all share that one Copy
button. Clicking it produced no "Copied" feedback and put nothing on the
clipboard.

## Why it failed

Two issues stacked:

1. The Tauri `clipboard-manager` plugin's `:default` permission set is
   **empty** — the plugin grants no clipboard access by default ("clipboard
   interaction needs to be explicitly enabled"). The desktop capability only
   listed `clipboard-manager:default`, so the `writeText` IPC call was denied
   by the ACL.
2. `InlineThread.svelte` was the lone component calling the raw plugin
   `writeText` with **no fallback**. The denied call threw before the
   `justCopied = true` line, so neither the feedback flag nor the clipboard
   write ever ran. The other copy buttons in the app route through the
   `copyToClipboard` helper (which falls back to `navigator.clipboard`), which
   masked the dead plugin path everywhere else.

## The fix

- `crates/er-desktop/capabilities/default.json` — replaced the empty
  `clipboard-manager:default` with `clipboard-manager:allow-write-text`,
  authorizing the plugin write path app-wide. Write-only: there is no
  `readText` usage, so the grant is minimal.
- `desktop-ui/src/lib/components/InlineThread.svelte` — routed Copy through the
  shared `copyToClipboard` helper instead of the raw plugin `writeText`,
  matching the other copy buttons and keeping Copy working in storybook /
  non-Tauri contexts via the `navigator.clipboard` fallback.

The capability is compiled into the binary by `tauri-build`, so the grant takes
effect after a rebuild.
