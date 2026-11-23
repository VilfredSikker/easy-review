import { beforeEach, describe, expect, it, mock } from "bun:test";
import type { AppSnapshot } from "$lib/pageTypes";

const calls: [string, Record<string, unknown>][] = [];
mock.module("@tauri-apps/api/core", () => ({
  invoke: (command: string, args: Record<string, unknown>) => {
    calls.push([command, args]);
    return Promise.resolve("");
  },
}));

const commands = await import("./tabCommands");

describe("the keyed direct commands", () => {
  beforeEach(() => {
    calls.length = 0;
    commands.nameShownTab(() => 7);
  });

  it("each names the tab the page shows", async () => {
    await commands.openSource("open_source", { path: "a.rs", line: 3 });
    await commands.openSource("open_in_vscode", { path: null, line: null });
    await commands.revealErFolder();
    await commands.lineCommits("a.rs", "k");
    await commands.exportReview({});
    await commands.exportReviewToFile({ includeChecklist: true });
    await commands.exportToAgent();
    expect(calls).toEqual([
      ["open_source", { tab: 7, path: "a.rs", line: 3 }],
      ["open_in_vscode", { tab: 7, path: null, line: null }],
      ["reveal_er_folder", { tab: 7 }],
      ["line_commits", { tab: 7, path: "a.rs", cacheKey: "k" }],
      ["export_review", { tab: 7, opts: {} }],
      ["export_review_to_file", { tab: 7, opts: { includeChecklist: true }, path: "" }],
      ["export_to_agent", { tab: 7 }],
    ]);
  });

  it("opens the selected file at its current hunk's first new line", () => {
    const snap = {
      selected_file: 1,
      current_hunk: 1,
      files: [
        { path: "a.rs", hunks: [] },
        { path: "b.rs", hunks: [{ new_start: 4 }, { new_start: 20 }] },
      ],
    } as unknown as AppSnapshot;
    expect(commands.selectedSource(snap)).toEqual({ path: "b.rs", line: 20 });
    expect(commands.selectedSource({ ...snap, current_hunk: 5 })).toEqual({ path: "b.rs", line: null });
    expect(commands.selectedSource(null)).toEqual({ path: null, line: null });
  });
});
