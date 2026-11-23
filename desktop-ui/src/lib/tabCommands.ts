// The direct commands that act on one tab: open its file in the editor,
// reveal its review bucket, attribute a file's lines, export its review. Each
// names the tab the page shows by its key, which the flag-on commands read the
// tab's content by; the flag-off ones read the App's tab and ignore it.

import { invoke } from "@tauri-apps/api/core";
import type { ExportOptions } from "$lib/protocol/generated/ExportOptions";
import type { LineCommits } from "$lib/protocol/generated/LineCommits";
import type { OpenSource } from "$lib/protocol/generated/OpenSource";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { AppSnapshot } from "$lib/types";

let activeTab: () => TabKey | null = () => null;

/** Names the tab the commands act on; the tabs adapter registers it at startup. */
export function nameShownTab(active: () => TabKey | null): void {
  activeTab = active;
}

const shown = () => ({ tab: activeTab() });

type SourceAt = { path: string | null; line: number | null };

/** The selected file, at the first new-side line of its current hunk. */
export function selectedSource(snap: AppSnapshot | null): SourceAt {
  const file = snap?.files[snap.selected_file];
  if (!snap || !file) return { path: null, line: null };
  return { path: file.path, line: file.hunks[snap.current_hunk ?? 0]?.new_start ?? null };
}

/**
 * Opens `at` in the editor (`open_source`, falling back to GitHub) or in VS
 * Code. No path answers `needs_checkout`.
 */
export function openSource(command: "open_source" | "open_in_vscode", at: SourceAt): Promise<OpenSource> {
  return invoke<OpenSource>(command, { ...shown(), path: at.path, line: at.line });
}

export function revealErFolder(): Promise<void> {
  return invoke<void>("reveal_er_folder", shown());
}

export function lineCommits(path: string, cacheKey: string): Promise<LineCommits> {
  return invoke<LineCommits>("line_commits", { ...shown(), path, cacheKey });
}

/** The tab's review export as markdown. */
export function exportReview(opts: ExportOptions): Promise<string> {
  return invoke<string>("export_review", { ...shown(), opts });
}

/** Writes the export to `path`, or to the tab's `export.md` when it is empty; answers the path. */
export function exportReviewToFile(opts: ExportOptions, path = ""): Promise<string> {
  return invoke<string>("export_review_to_file", { ...shown(), opts, path });
}

/** Writes the export, every option at its default, to the tab's `export.md`; answers the path. */
export function exportToAgent(): Promise<string> {
  return invoke<string>("export_to_agent", shown());
}
