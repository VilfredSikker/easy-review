import { copyToClipboard } from "$lib/clipboard";
import { asErError } from "$lib/protocol/client";
import type { ExportOptions } from "$lib/protocol/generated/ExportOptions";
import { exportReview, exportToAgent } from "$lib/tabCommands";
import { app } from "./app.svelte";

/**
 * Options for `export_review`, sent as-is. Fields left out take the engine's
 * serde defaults (every category on, checklist off), so the default here only
 * turns the checklist on to match what the Export page starts with.
 */
export type QuickExportOpts = ExportOptions;

const EXPORT_PAGE_DEFAULTS: QuickExportOpts = { includeChecklist: true };

/** Copy the review export to the clipboard without opening the Export page. */
export async function copyReviewExport(opts: QuickExportOpts = EXPORT_PAGE_DEFAULTS): Promise<void> {
  try {
    const body = await exportReview(opts);
    if (!body.trim()) {
      app.showToast("info", "Nothing to export yet");
      return;
    }
    await copyToClipboard(body);
    const lines = body.split("\n").length;
    app.showToast("success", `Export copied · ${lines} lines of markdown`);
  } catch (e) {
    const message = asErError(e).message;
    app.pushLog("error", "export_review", message);
    app.showToast("error", `Export failed: ${message}`);
  }
}

/** Write the review export, every option at its default, to the tab's `export.md`. */
export async function exportReviewToFile(): Promise<void> {
  try {
    const path = await exportToAgent();
    app.showToast("success", `Export written to ${path}`);
  } catch (e) {
    const message = asErError(e).message;
    app.pushLog("error", "export_to_agent", message);
    app.showToast("error", `Export failed: ${message}`);
  }
}
