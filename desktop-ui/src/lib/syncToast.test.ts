import { describe, expect, test } from "bun:test";
import { syncToastMessage } from "./syncToast";
import type { DiffStaleSnapshot, FileSnapshot } from "./types";

const file = (path: string, reviewed: boolean) => ({ path, reviewed }) as FileSnapshot;
const stale: DiffStaleSnapshot = {
  kind: "pr_head",
  message: "origin moved",
  built_from_oid: "1111111aaaa",
  latest_oid: "abcdef0123456",
} as DiffStaleSnapshot;

describe("syncToastMessage", () => {
  test("names the head a stale diff synced to", () => {
    expect(syncToastMessage({ files: [], diff_stale: stale }, { files: [], reviewed_cleared: 0 })).toBe(
      "Synced to abcdef0",
    );
  });

  test("a refresh with nothing stale says so plainly", () => {
    expect(syncToastMessage({ files: [], diff_stale: null }, { files: [] })).toBe("Diff refreshed");
  });

  test("folds the cleared Reviewed marks into the same toast", () => {
    expect(syncToastMessage({ files: [], diff_stale: stale }, { files: [], reviewed_cleared: 3 })).toBe(
      "Synced to abcdef0 · Reviewed cleared on 3 changed files",
    );
    expect(syncToastMessage({ files: [], diff_stale: null }, { files: [], reviewed_cleared: 1 })).toBe(
      "Diff refreshed · Reviewed cleared on 1 changed file",
    );
  });

  test("falls back to comparing snapshots when the engine count is missing", () => {
    const before = { files: [file("a.ts", true), file("b.ts", true), file("gone.ts", true)], diff_stale: null };
    const after = { files: [file("a.ts", false), file("b.ts", true)] };
    expect(syncToastMessage(before, after)).toBe("Diff refreshed · Reviewed cleared on 1 changed file");
  });

  test("the engine count wins over the snapshot comparison", () => {
    const before = { files: [file("a.ts", true)], diff_stale: null };
    expect(syncToastMessage(before, { files: [file("a.ts", false)], reviewed_cleared: 0 })).toBe("Diff refreshed");
  });
});
