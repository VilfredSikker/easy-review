import { invoke } from "@tauri-apps/api/core";
import { copyToClipboard } from "$lib/clipboard";
import { app } from "./app.svelte";

/**
 * Options for `export_review`, sent as-is. Fields left out take the engine's
 * serde defaults (every category on, checklist off), so the default here only
 * turns the checklist on to match what the Export page starts with.
 */
export type QuickExportOpts = Record<string, unknown>;

const EXPORT_PAGE_DEFAULTS: QuickExportOpts = { includeChecklist: true };

/** Copy the review export to the clipboard without opening the Export page. */
export async function copyReviewExport(opts: QuickExportOpts = EXPORT_PAGE_DEFAULTS): Promise<void> {
  try {
    const body = await invoke<string>("export_review", { opts });
    if (!body.trim()) {
      app.showToast("info", "Nothing to export yet");
      return;
    }
    await copyToClipboard(body);
    const lines = body.split("\n").length;
    app.showToast("success", `Export copied · ${lines} lines of markdown`);
  } catch (e) {
    app.pushLog("error", "export_review", String(e));
    app.showToast("error", `Export failed: ${e}`);
  }
}
