import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { app } from "./app.svelte";
import { diffSel } from "./diffSelection.svelte";
import { refHighlight } from "./referenceHighlight.svelte";
import { searchPrefillFromSelection } from "$lib/referenceHighlight";
import { terminal } from "./terminal.svelte";
import { browser } from "./browser.svelte";
import { openPrUrlModal } from "$lib/stores/prUrlModal.svelte";
import { overlay } from "./overlay.svelte";
import { commandPalette } from "./commandPalette.svelte";
import { buildTree, flattenForNav } from "$lib/treeFromPaths";
import { fileTreeCollapse } from "$lib/stores/fileTreeCollapse.svelte";
import { diffNav } from "$lib/stores/diffNav.svelte";
import { rightRail } from "$lib/stores/rightRail.svelte";

interface OpenSourceResult {
  kind: string;
  target: string;
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
 * Cmd+F prefill: the diff's active text selection (first line, trimmed,
 * length-capped — see `searchPrefillFromSelection`). Only selections inside
 * the diff scroll viewport (`.vscroll`) qualify; selections in side panels,
 * comments, or inputs fall back to the identifier-highlight prefill.
 */
function diffSelectionPrefill(): string | null {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) return null;
  const node = sel.getRangeAt(0).commonAncestorContainer;
  const el = node instanceof Element ? node : node.parentElement;
  if (!el?.closest(".vscroll")) return null;
  return searchPrefillFromSelection(sel.toString());
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

function togglePanelForKey(e: KeyboardEvent): boolean {
  if (e.key === "[" || e.code === "BracketLeft") {
    app.togglePanel("left");
    e.preventDefault();
    return true;
  }
  if (e.key === "]" || e.code === "BracketRight") {
    rightRail.toggle();
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
  const order = flattenForNav(tree, fileTreeCollapse.collapsed);
  if (order.length === 0) return;
  const cur = snap.files[snap.selected_file]?.path;
  let i = cur ? order.indexOf(cur) : -1;
  if (i === -1) i = 0;
  const nextPath = order[(i + direction + order.length) % order.length];
  const next = snap.files.find((f) => f.path === nextPath);
  if (!next) return;
  void app.cmd("select_file", { idx: next.source_index }).then(() => {
    void diffNav.scrollToFile(next.path);
  });
}

async function openInVsCode() {
  try {
    const result = await invoke<OpenSourceResult>("open_in_vscode");
    if (result.kind === "needs_checkout") {
      app.showToast("info", result.target);
    }
  } catch (e) {
    app.pushLog("error", "open_in_vscode", String(e));
    app.showToast("error", `VS Code: ${e}`);
  }
}

interface KeyContext {
  target: HTMLElement;
  inField: boolean;
  modalOpen: boolean;
}

/** One shortcut: acts and returns true when it owns the key, else false. */
type Shortcut = (e: KeyboardEvent, ctx: KeyContext) => boolean;

const withMod = (e: KeyboardEvent) => e.metaKey || e.ctrlKey;
const isLetter = (e: KeyboardEvent, lower: string) => e.key === lower || e.key === lower.toUpperCase();

/** Stop after preventDefault; the shared tail of most shortcuts. */
function handled(e: KeyboardEvent): true {
  e.preventDefault();
  return true;
}

function handleEscape(e: KeyboardEvent): boolean {
  if (e.key !== "Escape") return false;
  // Palette owns Escape (blur search, submenu back, or close).
  if (commandPalette.open) return true;
  if (overlay.dismissTopModal()) return handled(e);
  // Esc precedence: usages popover → Cmd+F search bar → diff selection →
  // identifier highlight.
  if (refHighlight.popoverOpen) {
    refHighlight.closePopover();
    return handled(e);
  }
  if (refHighlight.searchOpen) {
    refHighlight.closeSearch();
    return handled(e);
  }
  if (diffSel.active) {
    diffSel.clear();
    return handled(e);
  }
  if (refHighlight.active) {
    refHighlight.clear();
    return handled(e);
  }
  if (dismissBrowserAnnotationComposer) {
    dismissBrowserAnnotationComposer();
    return handled(e);
  }
  if (blurActiveField()) return handled(e);
  return false;
}

function selectTabByDigit(e: KeyboardEvent, { inField }: KeyContext): boolean {
  if (!withMod(e) || e.shiftKey || inField || !/^[1-9]$/.test(e.key)) return false;
  const tabIdx = parseInt(e.key, 10) - 1;
  const tabs = app.snapshot?.tabs ?? [];
  if (tabIdx < tabs.length) {
    e.preventDefault();
    app.cmd("select_tab", { idx: tabIdx });
  }
  return true;
}

/** Checked in order; the first that returns true ends the keydown. */
const GLOBAL_SHORTCUTS: Shortcut[] = [
  handleEscape,
  (e) => {
    if (!(e.ctrlKey && e.key === "q")) return false;
    getCurrentWindow().close();
    return true;
  },
  // Cmd/Ctrl+K belongs to the command palette.
  (e) => withMod(e) && e.key === "k",
  // Cmd/Ctrl+F opens the diff search bar. preventDefault suppresses the
  // webview's native find UI; works even when focus is in an input (the bar
  // refocuses its own field on open). An active text selection in the diff
  // prefills the query (priority over the identifier-highlight fallback).
  (e) => {
    if (!withMod(e) || e.shiftKey || !isLetter(e, "f")) return false;
    e.preventDefault();
    e.stopPropagation();
    refHighlight.openSearch(diffSelectionPrefill());
    return true;
  },
  (e, { inField }) => {
    if (!withMod(e) || e.shiftKey || !isLetter(e, "b")) return false;
    if (inField && !document.querySelector("[data-modal]")) return false;
    e.preventDefault();
    void browser.cycleLayout();
    return true;
  },
  (e, { inField }) => {
    if (!withMod(e) || !e.shiftKey || !isLetter(e, "b") || inField) return false;
    e.preventDefault();
    void browser.setLayout(browser.layout === "fullscreen" ? "hidden" : "fullscreen");
    return true;
  },
  (e) => {
    if (!withMod(e) || !e.shiftKey || !isLetter(e, "e")) return false;
    e.preventDefault();
    openExportReviewView();
    return true;
  },
  (e, { inField }) => {
    if (!withMod(e) || e.shiftKey || e.key !== "o" || inField) return false;
    e.preventDefault();
    app.cmd("open_worktree", {});
    return true;
  },
  (e) => {
    if (!withMod(e) || e.shiftKey || !isLetter(e, "p")) return false;
    e.preventDefault();
    focusSidebarSearchOrFileFilter();
    return true;
  },
  (e, { inField }) => {
    if (!withMod(e) || e.shiftKey || !isLetter(e, "t") || (inField && !terminal.open)) return false;
    e.preventDefault();
    terminal.toggle();
    return true;
  },
  (e, { inField }) => {
    if (!withMod(e) || !e.shiftKey || !isLetter(e, "t") || inField) return false;
    e.preventDefault();
    app.cmd("new_tab");
    return true;
  },
  (e, { inField }) => {
    if (!withMod(e) || e.shiftKey || !isLetter(e, "w") || inField) return false;
    e.preventDefault();
    const idx = app.snapshot?.active_tab ?? 0;
    app.cmd("close_tab", { idx });
    return true;
  },
  selectTabByDigit,
  (e) => {
    if (!withMod(e) || !e.shiftKey || !isLetter(e, "o")) return false;
    e.preventDefault();
    openPrUrlModal();
    return true;
  },
  (e, { inField, modalOpen }) => {
    const bare = !inField && !e.ctrlKey && !e.metaKey && !modalOpen;
    return (bare || withMod(e)) && togglePanelForKey(e);
  },
  (e, { target, inField, modalOpen }) => {
    if (e.key !== "`" || inField || modalOpen || target.closest(".xterm")) return false;
    e.preventDefault();
    terminal.toggle();
    return true;
  },
  (e) => {
    if (!withMod(e) || e.shiftKey || !isLetter(e, "r")) return false;
    e.preventDefault();
    app.cmd("force_refresh_diff");
    return true;
  },
];

/** Single-key shortcuts, only outside fields and modals and without modifiers. */
function handleBareKey(e: KeyboardEvent) {
  switch (e.key) {
    case "j":
      moveFile(1);
      break;
    case "k":
      moveFile(-1);
      break;
    case "/":
      e.preventDefault();
      focusInput('input[placeholder^="Filter files"]');
      break;
    case "d":
      e.preventDefault();
      app.toggleDiffViewMode();
      break;
    case "R":
      app.cmd("refresh_diff");
      break;
    case "e":
      void openInVsCode();
      break;
  }
}

export function initKeyboard(): () => void {
  function handler(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const inField =
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
      target.isContentEditable;
    const inTerminal = !!target.closest(".xterm");
    const modalOpen = !!document.querySelector("[data-modal]");

    if (inTerminal) {
      const isToggleTerminal = withMod(e) && !e.shiftKey && isLetter(e, "t");
      if (!isToggleTerminal) return;
    }

    const ctx = { target, inField, modalOpen };
    for (const shortcut of GLOBAL_SHORTCUTS) {
      if (shortcut(e, ctx)) return;
    }

    if (inField) return;
    if (document.querySelector("[data-modal]")) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    handleBareKey(e);
  }

  window.addEventListener("keydown", handler, { capture: true });
  return () => window.removeEventListener("keydown", handler, { capture: true });
}
