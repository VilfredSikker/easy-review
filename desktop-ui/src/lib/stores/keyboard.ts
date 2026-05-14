import { getCurrentWindow } from "@tauri-apps/api/window";
import { app } from "./app.svelte";
import { diffSel } from "./diffSelection.svelte";
import { terminal } from "./terminal.svelte";
import { browser } from "./browser.svelte";
import { openExportModal } from "$lib/components/ExportModal.svelte";
import { buildTree, flattenForNav } from "$lib/treeFromPaths";

/**
 * Buffer for the `gg` (jump to top) two-key sequence. Cleared after 600ms
 * or after any non-`g` key.
 */
let gBuffer = 0;
let gTimer: ReturnType<typeof setTimeout> | null = null;

function startSelectionFromHunk(kind: "comment" | "question") {
  const snap = app.snapshot;
  if (!snap) return;
  const file = snap.files[snap.selected_file];
  if (!file) return;
  const hunkIdx = snap.current_hunk ?? 0;
  const hunk = file.hunks[hunkIdx];
  if (!hunk) return;
  const firstLine = hunk.lines.find((l) => l.new_num !== null) ?? hunk.lines[0];
  const ln = firstLine?.new_num ?? firstLine?.old_num ?? hunk.new_start;
  diffSel.kind = kind;
  diffSel.start = ln;
  diffSel.end = ln;
  diffSel.file = file.path;
}

function focusInput(selector: string) {
  const el = document.querySelector<HTMLInputElement>(selector);
  if (el) {
    el.focus();
    el.select();
  }
}

/**
 * Move file selection in visual tree order (not flat `files` order, which is
 * path-sorted from `git diff`). The snapshot's `files` already reflects any
 * active filter, so we build the tree directly from it.
 */
function moveFile(direction: 1 | -1) {
  const snap = app.snapshot;
  if (!snap || snap.files.length === 0) return;
  const tree = buildTree(snap.files);
  const order = flattenForNav(tree);
  if (order.length === 0) return;
  const cur = snap.files[snap.selected_file]?.path;
  let i = cur ? order.indexOf(cur) : -1;
  if (i === -1) i = 0;
  const nextPath = order[(i + direction + order.length) % order.length];
  const idx = snap.files.findIndex((f) => f.path === nextPath);
  if (idx >= 0) app.cmd("select_file", { idx });
}

/**
 * In continuous-scroll mode, `next_hunk`/`prev_hunk` are file-local in the
 * engine (see er-engine/src/app/state/navigation.rs). When we're at the
 * boundary, transition to the adjacent file and reset to its first/last hunk.
 * Also scroll the focused hunk into view in the diff scroll area.
 */
function nextHunkAcrossFiles() {
  const snap = app.snapshot;
  if (!snap) return;
  const file = snap.files[snap.selected_file];
  const cur = snap.current_hunk ?? 0;
  const lastHunk = (file?.hunks.length ?? 0) - 1;
  if (file && cur < lastHunk) {
    app.cmd("next_hunk");
    scrollFocusedHunkIntoView();
    return;
  }
  // At last hunk — jump to next file's first hunk.
  const nextIdx = snap.selected_file + 1;
  if (nextIdx < snap.files.length) {
    app.cmd("select_file", { idx: nextIdx });
    // next_hunk on a fresh file leaves current_hunk at 0 by default — explicitly
    // ensure we're at hunk 0 by no-op'ing (select_file resets current_hunk to 0
    // in the engine). Scroll into view.
    setTimeout(scrollFocusedHunkIntoView, 0);
  }
}

function prevHunkAcrossFiles() {
  const snap = app.snapshot;
  if (!snap) return;
  const cur = snap.current_hunk ?? 0;
  if (cur > 0) {
    app.cmd("prev_hunk");
    scrollFocusedHunkIntoView();
    return;
  }
  // At first hunk — jump to previous file's last hunk.
  const prevIdx = snap.selected_file - 1;
  if (prevIdx >= 0) {
    const prevFile = snap.files[prevIdx];
    const lastHunk = Math.max(0, (prevFile?.hunks.length ?? 1) - 1);
    app.cmd("select_file", { idx: prevIdx });
    // Walk forward to the last hunk if there are any.
    for (let i = 0; i < lastHunk; i++) {
      app.cmd("next_hunk");
    }
    setTimeout(scrollFocusedHunkIntoView, 0);
  }
}

function scrollFocusedHunkIntoView() {
  const snap = app.snapshot;
  if (!snap) return;
  const file = snap.files[snap.selected_file];
  if (!file) return;
  // Anchor on the file section — the per-file sticky header keeps the user
  // oriented even when only the file changes.
  document
    .getElementById(`file-${file.path}`)
    ?.scrollIntoView({ behavior: "smooth", block: "start" });
}

function scrollDiff(to: "top" | "bottom") {
  const el = document.querySelector<HTMLElement>(".mono.text-\\[13px\\]");
  if (!el) return;
  el.scrollTo({ top: to === "top" ? 0 : el.scrollHeight, behavior: "smooth" });
}

export function initKeyboard(): () => void {
  function handler(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const inField =
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
      target.isContentEditable;

    // Always-fire shortcuts (work even when focus is in a text field):
    // Esc dismisses the diff composer.
    if (e.key === "Escape" && diffSel.active) {
      diffSel.clear();
      e.preventDefault();
      return;
    }
    // ⌘Q / Ctrl+Q closes the window.
    if (e.ctrlKey && e.key === "q") {
      getCurrentWindow().close();
      return;
    }
    // ⌘K / Ctrl+K opens the palette (palette has its own handler too).
    if ((e.metaKey || e.ctrlKey) && e.key === "k") {
      return;
    }
    // ⌘B — toggle browser view.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "b" || e.key === "B") && !inField) {
      e.preventDefault();
      browser.toggleOpen();
      return;
    }
    // ⌘⇧E — open the export-review modal.
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "e" || e.key === "E")) {
      e.preventDefault();
      openExportModal();
      return;
    }
    // ⌘O — open a local repo (EmptyState shortcut). Gated on !inField so
    // typing "o" with ⌘ held in a textarea doesn't pop a folder picker.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key === "o" && !inField) {
      e.preventDefault();
      app.cmd("open_worktree", {});
      return;
    }
    // ⌘T — new tab.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "t" || e.key === "T") && !inField) {
      e.preventDefault();
      app.cmd("new_tab");
      return;
    }
    // ⌘W — close active tab.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "w" || e.key === "W") && !inField) {
      e.preventDefault();
      const idx = app.snapshot?.active_tab ?? 0;
      app.cmd("close_tab", { idx });
      return;
    }
    // ⌘1..9 — jump to the Nth tab.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !inField && /^[1-9]$/.test(e.key)) {
      const target = parseInt(e.key, 10) - 1;
      const tabs = app.snapshot?.tabs ?? [];
      if (target < tabs.length) {
        e.preventDefault();
        app.cmd("select_tab", { idx: target });
      }
      return;
    }
    // ⌘⇧O — open a PR by URL. No direct dialog yet; fires the EmptyState
    // paste field via a "focus" pseudo-event by toggling the right route.
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "o" || e.key === "O")) {
      e.preventDefault();
      const input = document.querySelector<HTMLInputElement>('input[placeholder*="GitHub PR URL"]');
      input?.focus();
      return;
    }
    // Panel toggles fire from anywhere — `[`, `\`, `]` aren't valid edit input
    // alone, and these are app-level toggles users expect to always work.
    if (!e.ctrlKey && !e.metaKey && !e.altKey) {
      if (e.key === "[") { app.togglePanel("left"); e.preventDefault(); return; }
      if (e.key === "]") { app.togglePanel("right"); e.preventDefault(); return; }
      if (e.key === "\\") { app.togglePanel("tree"); e.preventDefault(); return; }
    }
    // Backtick / Cmd+` toggles the terminal drawer. We let it fire even when
    // focus is in an input field that isn't the terminal itself — the terminal
    // captures its own keystrokes via xterm's input handling, not via the DOM
    // event we see here.
    if (e.key === "`" && !target.closest(".xterm")) {
      e.preventDefault();
      terminal.toggle();
      return;
    }

    // Below: only when NOT typing in a field.
    if (inField) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;

    // Track the `g` buffer for `gg`. Reset on any non-`g` key.
    if (e.key !== "g" && gBuffer > 0) {
      gBuffer = 0;
      if (gTimer) { clearTimeout(gTimer); gTimer = null; }
    }

    switch (e.key) {

      // "?" — open command palette (acts as "all shortcuts" entry).
      case "?": {
        e.preventDefault();
        const ev = new KeyboardEvent("keydown", { key: "k", metaKey: true });
        window.dispatchEvent(ev);
        break;
      }

      // File navigation
      case "j": moveFile(1); break;
      case "k": moveFile(-1); break;
      case "U": app.cmd("jump_to_unreviewed"); break;

      // Hunk navigation — crosses file boundaries in continuous-scroll mode.
      case "n": nextHunkAcrossFiles(); break;
      case "N": prevHunkAcrossFiles(); break;
      case "Tab":
        e.preventDefault();
        if (e.shiftKey) prevHunkAcrossFiles();
        else nextHunkAcrossFiles();
        break;

      // Scroll-to-top / scroll-to-bottom (vim-style)
      case "g":
        if (gBuffer === 1) {
          gBuffer = 0;
          if (gTimer) { clearTimeout(gTimer); gTimer = null; }
          scrollDiff("top");
        } else {
          gBuffer = 1;
          if (gTimer) clearTimeout(gTimer);
          gTimer = setTimeout(() => { gBuffer = 0; gTimer = null; }, 600);
        }
        break;
      case "G":
        scrollDiff("bottom");
        break;

      // Toggle unified ↔ split diff view
      case "d":
        e.preventDefault();
        app.toggleDiffViewMode();
        break;

      // Reviewed
      case "r": app.cmd("toggle_reviewed"); break;
      case " ":
        app.cmd("toggle_reviewed");
        e.preventDefault();
        break;

      // Filter / search input focus
      case "/":
      case "f":
        e.preventDefault();
        focusInput('input[placeholder^="Filter files"]');
        break;

      // Composer shortcuts — open with current hunk pre-selected
      case "c":
        e.preventDefault();
        startSelectionFromHunk("comment");
        break;
      case "q":
        e.preventDefault();
        startSelectionFromHunk("question");
        break;

      // Expand / collapse compacted file
      case "Enter": app.cmd("toggle_compacted"); break;

      // Refresh diff
      case "R": app.cmd("refresh_diff"); break;

      // Open in editor
      case "e": {
        import("@tauri-apps/api/core").then(({ invoke }) => {
          invoke("open_in_editor").catch(() => {});
        });
        break;
      }

      // Scope / mode switching
      case "1": app.cmd("set_mode", { mode: "branch" }); break;
      case "2": app.cmd("set_mode", { mode: "unstaged" }); break;
      case "3": app.cmd("set_mode", { mode: "staged" }); break;
      case "4": app.cmd("set_mode", { mode: "history" }); break;
    }
  }

  window.addEventListener("keydown", handler, { capture: true });
  return () => window.removeEventListener("keydown", handler, { capture: true });
}
