import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { app } from "./app.svelte";
import { diffSel } from "./diffSelection.svelte";
import { refHighlight } from "./referenceHighlight.svelte";
import { terminal } from "./terminal.svelte";
import { browser } from "./browser.svelte";
import { closeAiActionPalette } from "$lib/components/AiActionPalette.svelte";
import { openPrUrlModal } from "$lib/stores/prUrlModal.svelte";
import { overlay } from "./overlay.svelte";
import { buildTree, flattenForNav } from "$lib/treeFromPaths";
import { fileTreeCollapse } from "$lib/stores/fileTreeCollapse.svelte";
import { diffNav } from "$lib/stores/diffNav.svelte";
import { rightRail } from "$lib/stores/rightRail.svelte";

interface OpenSourceResult {
  kind: string;
  target: string;
}

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

export function initKeyboard(): () => void {
  function handler(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const inField =
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) ||
      target.isContentEditable;
    const inTerminal = !!target.closest(".xterm");

    if (inTerminal) {
      const isToggleTerminal =
        (e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "t" || e.key === "T");
      if (!isToggleTerminal) return;
    }

    if (e.key === "Escape") {
      if (overlay.dismissTopModal()) {
        e.preventDefault();
        return;
      }
      closeAiActionPalette();
      // Usages popover closes before anything else; a second Esc then clears
      // the highlight itself (next branch below).
      if (refHighlight.popoverOpen) {
        refHighlight.closePopover();
        e.preventDefault();
        return;
      }
      if (diffSel.active) {
        diffSel.clear();
        e.preventDefault();
        return;
      }
      if (refHighlight.active) {
        refHighlight.clear();
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
    if (e.ctrlKey && e.key === "q") {
      getCurrentWindow().close();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === "k") {
      return;
    }
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "a" || e.key === "A")) {
      e.preventDefault();
      e.stopPropagation();
      openAiPaletteCallback?.();
      return;
    }
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
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "b" || e.key === "B") && !inField) {
      e.preventDefault();
      void browser.setLayout(browser.layout === "fullscreen" ? "hidden" : "fullscreen");
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "e" || e.key === "E")) {
      e.preventDefault();
      openExportReviewView();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key === "o" && !inField) {
      e.preventDefault();
      app.cmd("open_worktree", {});
      return;
    }
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "p" || e.key === "P")) {
      e.preventDefault();
      focusSidebarSearchOrFileFilter();
      return;
    }
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
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "t" || e.key === "T") && !inField) {
      e.preventDefault();
      app.cmd("new_tab");
      return;
    }
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "w" || e.key === "W") && !inField) {
      e.preventDefault();
      const idx = app.snapshot?.active_tab ?? 0;
      app.cmd("close_tab", { idx });
      return;
    }
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !inField && /^[1-9]$/.test(e.key)) {
      const target = parseInt(e.key, 10) - 1;
      const tabs = app.snapshot?.tabs ?? [];
      if (target < tabs.length) {
        e.preventDefault();
        app.cmd("select_tab", { idx: target });
      }
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "o" || e.key === "O")) {
      e.preventDefault();
      openPrUrlModal();
      return;
    }
    if (
      ((!inField && !e.ctrlKey && !e.metaKey) || (e.metaKey || e.ctrlKey)) &&
      togglePanelForKey(e)
    ) {
      return;
    }
    if (e.key === "`" && !target.closest(".xterm")) {
      e.preventDefault();
      terminal.toggle();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && (e.key === "r" || e.key === "R")) {
      e.preventDefault();
      app.cmd("force_refresh_diff");
      return;
    }

    if (inField) return;
    if (document.querySelector("[data-modal]")) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;

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

  window.addEventListener("keydown", handler, { capture: true });
  return () => window.removeEventListener("keydown", handler, { capture: true });
}
