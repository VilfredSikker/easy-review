import { describe, expect, it } from "bun:test";
import { resolveOmittedHunks } from "./snapshotDelta";
import type { AppSnapshot, FileSnapshot, HunkSnapshot } from "$lib/types";

function hunk(text: string): HunkSnapshot {
  return {
    header: "@@ -1 +1 @@",
    old_start: 1,
    old_count: 1,
    new_start: 1,
    new_count: 1,
    lines: [{ old_num: 1, new_num: 1, kind: "context", text }],
    threads: [],
  } as unknown as HunkSnapshot;
}

function file(path: string, over: Partial<FileSnapshot> = {}): FileSnapshot {
  return {
    path,
    status: "modified",
    additions: 1,
    deletions: 0,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks: [hunk(`line of ${path}`)],
    source_index: 0,
    cache_key: "ck",
    delta_key: "dk-" + path,
    ...over,
  } as FileSnapshot;
}

function snap(files: FileSnapshot[]): AppSnapshot {
  return { files } as unknown as AppSnapshot;
}

describe("resolveOmittedHunks", () => {
  it("splices hunks from prev when delta_key matches", () => {
    const prev = snap([file("a.rs")]);
    const next = snap([file("a.rs", { hunks: [], hunks_omitted: true })]);
    const stats = resolveOmittedHunks(prev, next);
    expect(stats.reused).toBe(1);
    expect(stats.refetch).toBe(0);
    expect(next.files[0].hunks.length).toBe(1);
    expect(next.files[0].hunks_omitted).toBe(false);
    expect(next.files[0].is_lazy_stub).toBeFalsy();
    // Reuses the same array reference — keeps downstream block caches warm.
    expect(next.files[0].hunks).toBe(prev.files[0].hunks);
  });

  it("downgrades to lazy stub when delta_key differs", () => {
    const prev = snap([file("a.rs", { delta_key: "old" })]);
    const next = snap([file("a.rs", { hunks: [], hunks_omitted: true, delta_key: "new" })]);
    const stats = resolveOmittedHunks(prev, next);
    expect(stats.refetch).toBe(1);
    expect(next.files[0].is_lazy_stub).toBe(true);
    expect(next.files[0].hunks.length).toBe(0);
  });

  it("downgrades to lazy stub when prev is missing or empty", () => {
    const next = snap([file("a.rs", { hunks: [], hunks_omitted: true })]);
    expect(resolveOmittedHunks(null, next).refetch).toBe(1);
    expect(next.files[0].is_lazy_stub).toBe(true);

    const next2 = snap([file("b.rs", { hunks: [], hunks_omitted: true })]);
    const prevStub = snap([file("b.rs", { hunks: [], is_lazy_stub: true })]);
    expect(resolveOmittedHunks(prevStub, next2).refetch).toBe(1);
  });

  it("leaves non-omitted files untouched", () => {
    const prev = snap([file("a.rs", { delta_key: "old" })]);
    const fresh = file("a.rs", { delta_key: "new" });
    const freshHunks = fresh.hunks;
    const next = snap([fresh]);
    const stats = resolveOmittedHunks(prev, next);
    expect(stats.reused).toBe(0);
    expect(stats.refetch).toBe(0);
    expect(next.files[0].hunks).toBe(freshHunks);
  });
});
