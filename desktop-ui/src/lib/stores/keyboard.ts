import { getCurrentWindow } from "@tauri-apps/api/window";
import { app } from "./app.svelte";
import { diffSel } from "./diffSelection.svelte";
import { terminal } from "./terminal.svelte";
import { browser } from "./browser.svelte";
import { closeAiActionPalette } from "$lib/components/AiActionPalette.svelte";
import { closePrUrlModal, isPrUrlModalOpen, openPrUrlModal } from "$lib/stores/prUrlModal.svelte";
import { buildTree, flattenForNav } from "$lib/treeFromPaths";
import { diffNav } from "$lib/stores/diffNav.svelte";

// Callbacks registered by AiActionPalette to open itself
let openAiPaletteCallback: (() => void) | null = null;

export function registerAiPaletteOpener(fn: () => void): () => void {
  openAiPaletteCallback = fn;
  return () => { if (openAiPaletteCallback === fn) openAiPaletteCallback = null; };
}

export function triggerAiPalette(): void {
  void openAiPaletteCallback?.();
}

function openExportReviewView(): void {
  app.setMainView("export-review");
  if (browser.layout === "fullscreen") void browser.setLayout("hidden");
}

let dismissBrowserAnnotationComposer: (() => void) | null = null;

/** AnnotationOverlay registers while the browser note composer is open. */
export function registerBrowserAnnotationComposerDismiss(fn: (() => void) | null): void {
  dismissBrowserAnnotationComposer = fn;
}

/** Close the in-page annotation composer (e.g. after deleting from the side panel). */
export function dismissBrowserAnnotationComposerNow(): void {
  dismissBrowserAnnotationComposer?.();
}

function blurActiveField(): boolean {
  const el = document.activeElement as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  if (["INPUT", "TEXTAREA", "SELECT"].includes(tag) || el.isContentEditable) {
    el.blur();
    return true;
  }
  return false;
}

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

function focusSidebarSearchOrFileFilter() {
  const sidebarInput = document.querySelector<HTMLInputElement>("[data-left-sidebar-search-input]");
  if (sidebarInput) {
    sidebarInput.focus();
    sidebarInput.select();
    return;
  }
  focusInput('input[placeholder^="Filter files"]');
}

function isPlainShortcut(e: KeyboardEvent): boolean {
  return !e.ctrlKey && !e.metaKey && !e.altKey;
}

function isCommandShortcut(e: KeyboardEvent): boolean {
  return e.metaKey || e.ctrlKey;
}

function isReviewPanelShortcut(e: KeyboardEvent, inField: boolean): boolean {
  return !inField && !e.ctrlKey && !e.metaKey;
}

function togglePanelForKey(e: KeyboardEvent): boolean {
  if (e.key === "[" || e.code === "BracketLeft") {
    app.togglePanel("left");
    e.preventDefault();
    return true;
  }
  if (e.key === "]" || e.code === "BracketRight") {
    app.togglePanel("right");
    e.preventDefault();
    return true;
  }
  if (e.key === "\\" || e.code === "Backslash") {
    app.togglePanel("tree");
    e.preventDefault();
    return true;
  }
  return false;
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
  const next = snap.files.find((f) => f.path === nextPath);
  if (next) app.cmd("select_file", { idx: next.source_index });
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
    // Use cur+1 — snapshot hasn't updated yet when we scroll.
    diffNav.scrollToHunk(file.path, cur + 1);
    return;
  }
  // At last hunk — jump to next file's first hunk.
  const nextVisibleIdx = snap.selected_file + 1;
  if (nextVisibleIdx < snap.files.length) {
    const nextFile = snap.files[nextVisibleIdx];
    app.cmd("select_file", { idx: nextFile.source_index });
    diffNav.scrollToHunk(nextFile.path, 0);
  }
}

function prevHunkAcrossFiles() {
  const snap = app.snapshot;
  if (!snap) return;
  const file = snap.files[snap.selected_file];
  const cur = snap.current_hunk ?? 0;
  if (file && cur > 0) {
    app.cmd("prev_hunk");
    // Use cur-1 — snapshot hasn't updated yet when we scroll.
    diffNav.scrollToHunk(file.path, cur - 1);
    return;
  }
  // At first hunk — jump to previous file's last hunk.
  const prevVisibleIdx = snap.selected_file - 1;
  if (prevVisibleIdx >= 0) {
    const prevFile = snap.files[prevVisibleIdx];
    const lastHunk = Math.max(0, (prevFile?.hunks.length ?? 1) - 1);
    app.cmd("select_file", { idx: prevFile.source_index });
    for (let i = 0; i < lastHunk; i++) {
      app.cmd("next_hunk");
    }
    diffNav.scrollToHunk(prevFile.path, lastHunk);
  }
}

function scrollDiff(to: "top" | "bottom") {
  diffNav.scrollToEdge(to);
}

export function initKeyboard(): () => void {
  function handler(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const inField =
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
      target.isContentEditable;
    const inTerminal = !!target.closest(".xterm");

    // When focus is inside xterm, every keystroke belongs to the PTY. Capture-phase
    // preventDefault() here would swallow input before xterm's onData fires. Only
    // Cmd/Ctrl+T is allowed through so the terminal toggle still closes the drawer.
    if (inTerminal) {
      const isToggleTerminal =
        (e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "t" || e.key === "T");
      if (!isToggleTerminal) return;
    }

    // Always-fire shortcuts (work even when focus is in a text field):
    // Esc dismisses AI Hub, diff composer, then blurs focused inputs.
    if (e.key === "Escape") {
      if (document.querySelector("[data-modal]")) {
        return;
      }
      if (isPrUrlModalOpen()) {
        closePrUrlModal();
        e.preventDefault();
        return;
      }
      closeAiActionPalette();
      if (diffSel.active) {
        diffSel.clear();
        e.preventDefault();
        return;
      }
      if (dismissBrowserAnnotationComposer) {
        dismissBrowserAnnotationComposer();
        e.preventDefault();
        return;
      }
      if (blurActiveField()) {
        e.preventDefault();
        return;
      }
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
    // ⌘A / Ctrl+A — open AI action palette. Claimed globally (even from input
    // fields) so the shortcut is reliable; native select-all is sacrificed.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      e.stopPropagation();
      openAiPaletteCallback?.();
      return;
    }
    // ⌘B — cycle browser layout (also from modals, including while typing in export).
    if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey &&
      (e.key === "b" || e.key === "B") &&
      (!inField || !!document.querySelector("[data-modal]"))
    ) {
      e.preventDefault();
      void browser.cycleLayout();
      return;
    }
    // ⌘⇧B — fullscreen browser only.
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "b" || e.key === "B") && !inField) {
      e.preventDefault();
      void browser.setLayout(browser.layout === "fullscreen" ? "hidden" : "fullscreen");
      return;
    }
    // ⌘⇧E — open the export-review workspace view.
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "e" || e.key === "E")) {
      e.preventDefault();
      openExportReviewView();
      return;
    }
    // ⌘O — open a local repo (EmptyState shortcut). Gated on !inField so
    // typing "o" with ⌘ held in a textarea doesn't pop a folder picker.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key === "o" && !inField) {
      e.preventDefault();
      app.cmd("open_worktree", {});
      return;
    }
    // ⌘P — sidebar search (projects/branches/PRs), not file filter.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "p" || e.key === "P")) {
      e.preventDefault();
      focusSidebarSearchOrFileFilter();
      return;
    }
    // ⌘T — toggle terminal (Codex-style shortcut). Closing must work even
    // when xterm's hidden textarea has focus; opening from other text fields
    // still stays out of the way of native editing shortcuts.
    if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey &&
      (e.key === "t" || e.key === "T") &&
      (!inField || terminal.open)
    ) {
      e.preventDefault();
      terminal.toggle();
      return;
    }
    // ⌘⇧T — new tab.
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "t" || e.key === "T") && !inField) {
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
    // ⌘⇧O — open PR URL modal.
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "o" || e.key === "O")) {
      e.preventDefault();
      openPrUrlModal();
      return;
    }
    // Panel toggles. Plain keys stay review-only so typing in fields/terminal
    // is not hijacked. Cmd/Ctrl variants are app-level and work from focused
    // inputs, including xterm.
    if (
      (isReviewPanelShortcut(e, inField) || isCommandShortcut(e)) &&
      togglePanelForKey(e)
    ) {
      return;
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

    // Cmd/Ctrl+R — force-refresh diff source for local PR tabs.
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "r" || e.key === "R")) {
      e.preventDefault();
      app.cmd("force_refresh_diff");
      return;
    }

    // Below: only when NOT typing in a field or a modal overlay is open.
    if (inField) return;
    if (document.querySelector('[data-modal]')) return;
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
      case "Tab": {
        e.preventDefault();
        const tabs = app.snapshot?.tabs ?? [];
        if (tabs.length <= 1) break;
        const active = app.snapshot?.active_tab ?? 0;
        const next = e.shiftKey
          ? (active === 0 ? tabs.length - 1 : active - 1)
          : (active + 1) % tabs.length;
        app.cmd("select_tab", { idx: next });
        break;
      }

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
          invoke("open_source").catch(() => {});
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
