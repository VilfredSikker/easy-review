// What an export carries. One store, so the Export page, quick copy and every
// other "Copy export" entry point produce the same markdown.
//
// Category toggles and Only unresolved are a per-viewer preference in
// localStorage — never repo-local config (ADR 0005). Per-item exclusions stay
// in memory: item ids belong to one review, so a remembered exclusion would
// silently drop an item from a review nobody has looked at yet.

import { invoke } from "@tauri-apps/api/core";
import { copyToClipboard } from "$lib/clipboard";
import {
  buildExportOpts,
  collectExportItems,
  exportItemCount,
  exportTabKey,
  markdownLineCount,
  parseExportPrefs,
  type ExportCategory,
  type ExportOpts,
  type ExportPrefs,
} from "$lib/exportOptions";
import { app } from "./app.svelte";

export const EXPORT_PREFS_KEY = "exportOptions";

const NO_EXCLUSIONS: ReadonlySet<string> = new Set();

function loadPrefs(): ExportPrefs {
  try {
    return parseExportPrefs(typeof localStorage === "undefined" ? null : localStorage.getItem(EXPORT_PREFS_KEY));
  } catch {
    return parseExportPrefs(null);
  }
}

function persistPrefs(prefs: ExportPrefs) {
  try {
    localStorage.setItem(EXPORT_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* ignore quota / privacy-mode errors */
  }
}

class ExportOptionsStore {
  prefs = $state<ExportPrefs>(loadPrefs());
  private excludedByTab = $state<Record<string, ReadonlySet<string>>>({});

  readonly items = $derived(collectExportItems(app.snapshot?.ai));
  readonly excluded = $derived(this.excludedByTab[exportTabKey(app.snapshot)] ?? NO_EXCLUSIONS);
  readonly current: ExportOpts = $derived(buildExportOpts(this.prefs, this.items, this.excluded));
  readonly itemCount = $derived(
    exportItemCount(this.prefs, this.items, this.excluded, app.snapshot?.ui_annotations ?? []),
  );

  toggleCategory(key: ExportCategory) {
    this.prefs = { ...this.prefs, include: { ...this.prefs.include, [key]: !this.prefs.include[key] } };
    persistPrefs(this.prefs);
  }

  toggleOnlyUnresolved() {
    this.prefs = { ...this.prefs, onlyUnresolved: !this.prefs.onlyUnresolved };
    persistPrefs(this.prefs);
  }

  isExcluded(id: string): boolean {
    return this.excluded.has(id);
  }

  toggleItem(id: string) {
    const key = exportTabKey(app.snapshot);
    const ids = [...this.excluded];
    const next = this.excluded.has(id) ? ids.filter((x) => x !== id) : [...ids, id];
    // Replaced, never mutated: the record swap is what the deriveds track.
    this.excludedByTab = { ...this.excludedByTab, [key]: new Set(next) };
  }

  /** Copies the export with the current choices, from anywhere in the app. */
  async quickCopy(): Promise<void> {
    const count = this.itemCount;
    try {
      const body = await invoke<string>("export_review", { opts: this.current });
      await copyToClipboard(body);
      app.showToast("success", `Export copied · ${count} items, ${markdownLineCount(body)} lines of markdown`);
    } catch (e) {
      app.showToast("error", `Export failed: ${String(e)}`);
    }
  }
}

export const exportOptions = new ExportOptionsStore();
