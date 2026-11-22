import { describe, expect, it } from "bun:test";
import type { AppSnapshot } from "$lib/types";
import { SelectionAdapter } from "./selectionAdapter";

const KEY = 7;

/** Three shown files; `c.rs` is the only unreviewed one, and `a.rs` has two hunks. */
function snapshot(): AppSnapshot {
  const file = (path: string, source_index: number, reviewed: boolean, hunks = 1) => ({
    path,
    source_index,
    reviewed,
    hunks: Array.from({ length: hunks }, () => ({})),
  });
  const files = [file("a.rs", 4, true, 2), file("b.rs", 9, true), file("c.rs", 12, false)];
  return { files, selected_file: 0, current_hunk: 0 } as unknown as AppSnapshot;
}

function adapter(opts: { flagOn?: boolean; drawsFromDiff?: boolean } = {}) {
  const page: { snap: AppSnapshot } = { snap: snapshot() };
  const selection = new SelectionAdapter({
    flagOn: opts.flagOn ?? true,
    activeKey: () => KEY,
    drawsFromDiff: () => opts.drawsFromDiff ?? true,
    shown: () => page.snap,
  });
  /** Runs a command as `app.cmd` would, then shows the overlaid snapshot. */
  const run = (command: string, args: Record<string, unknown> = {}) => {
    const routed = selection.route(command, args);
    page.snap = selection.overlay(snapshot());
    return routed;
  };
  return { selection, page, run };
}

const at = (snap: AppSnapshot) => [snap.files[snap.selected_file]?.path, snap.current_hunk];

describe("SelectionAdapter", () => {
  it("leaves the App's selection with the flag off and in the views the App loads", () => {
    for (const opts of [{ flagOn: false }, { drawsFromDiff: false }]) {
      const { selection } = adapter(opts);
      const snap = snapshot();
      expect(selection.route("select_file", { idx: 9 })).toBeNull();
      expect(selection.overlay(snap)).toBe(snap);
    }
  });

  it("selects a file in the page, by its source index, with no IPC call", async () => {
    const { page, run } = adapter();
    const routed = run("select_file", { idx: 9 });
    expect(routed).not.toBeNull();
    await routed;
    expect(at(page.snap)).toEqual(["b.rs", 0]);
  });

  it("moves through files and hunks with the engine's rules", () => {
    const { page, run } = adapter();
    run("next_hunk");
    expect(at(page.snap)).toEqual(["a.rs", 1]);
    run("next_hunk");
    expect(at(page.snap)).toEqual(["a.rs", 1]);
    run("prev_file");
    expect(at(page.snap)).toEqual(["c.rs", 0]);
    run("next_file");
    expect(at(page.snap)).toEqual(["a.rs", 0]);
    run("jump_to_unreviewed");
    expect(at(page.snap)).toEqual(["c.rs", 0]);
  });
});
