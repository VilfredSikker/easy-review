import { describe, expect, it } from "bun:test";
import {
  annotationVersion,
  buildAnnotationIndex,
  fallbackFindings,
  fallbackThreadsForHunk,
  findingBelongsToHunk,
  findingRendersInline,
  findingsForLine,
  findingsForSplitRow,
  hunkLevelFindings,
  threadsForLine,
  type CommentVisibility,
} from "./diffAnnotations";
import type { FileSnapshot, FlatFinding, HunkSnapshot, LineSnapshot, ThreadSnapshot } from "./types";

const VIS_OFF: CommentVisibility = { hideAll: false, hideResolved: false, hideOutdated: false };

function mkLine(opts: Partial<LineSnapshot> & Pick<LineSnapshot, "kind">): LineSnapshot {
  return {
    old_num: null,
    new_num: null,
    text: "",
    ...opts,
  };
}

function mkThread(opts: Partial<ThreadSnapshot> & Pick<ThreadSnapshot, "id" | "file" | "line">): ThreadSnapshot {
  return {
    kind: "comment",
    source: "local",
    synced: false,
    stale: false,
    resolved: false,
    root: { id: opts.id + ":root", author: "me", kind: "you", timestamp: "", body_markdown: "" },
    replies: [],
    promoted_to: null,
    ...opts,
  };
}

function mkFinding(opts: Partial<FlatFinding> & Pick<FlatFinding, "id" | "file">): FlatFinding {
  return {
    line: null,
    hunk_index: null,
    severity: "med",
    title: "t",
    message_markdown: "",
    promoted_to: null,
    thread_id: null,
    ...opts,
  };
}

function mkHunk(opts: Partial<HunkSnapshot> & Pick<HunkSnapshot, "new_start" | "new_count" | "lines">): HunkSnapshot {
  return {
    header: "@@",
    old_start: opts.new_start,
    old_count: opts.new_count,
    threads: [],
    ...opts,
  };
}

function mkFile(path: string, hunks: HunkSnapshot[]): FileSnapshot {
  return {
    path,
    status: "modified",
    additions: 0,
    deletions: 0,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks,
    source_index: 0,
    cache_key: path,
  };
}

// ---- Fixtures ----

const FILE = "src/foo.ts";

// Hunk: new lines 10..14 (5 lines). Includes a del/add pair at new_num=11.
const hunkLines: LineSnapshot[] = [
  mkLine({ kind: "context", old_num: 9, new_num: 10, text: "ctx" }),
  mkLine({ kind: "del", old_num: 10, new_num: null, text: "old" }),       // del at old=10 (renders as 10)
  mkLine({ kind: "add", old_num: null, new_num: 11, text: "new" }),       // add at new=11 (lineNum=11)
  mkLine({ kind: "context", old_num: 11, new_num: 12, text: "ctx2" }),
  mkLine({ kind: "add", old_num: null, new_num: 13, text: "added" }),
];
const hunk = mkHunk({ new_start: 10, new_count: 4, lines: hunkLines });

const baseThread = mkThread({ id: "t1", file: FILE, line: 13 });
const resolvedThread = mkThread({ id: "t2", file: FILE, line: 13, resolved: true });
const staleThread = mkThread({ id: "t3", file: FILE, line: 13, stale: true });
const fallbackThread = mkThread({ id: "t4", file: FILE, line: 999 });
const ownedThread = mkThread({ id: "tOwned", file: FILE, line: 13 });

function buildFixture(): { ai: { threads: ThreadSnapshot[]; findings: FlatFinding[] }; files: FileSnapshot[] } {
  const findings: FlatFinding[] = [
    mkFinding({ id: "f-line-12", file: FILE, line: 12, hunk_index: 0 }),
    mkFinding({ id: "f-line-13", file: FILE, line: 13, hunk_index: 0 }),
    mkFinding({ id: "f-del-only", file: FILE, line: 10, hunk_index: 0 }), // del-only, no new_num match in hunk
    mkFinding({ id: "f-hunk", file: FILE, line: null, hunk_index: 0 }),
    mkFinding({ id: "f-owned", file: FILE, line: 13, hunk_index: 0, thread_id: "tOwned" }),
    mkFinding({ id: "f-outside", file: FILE, line: 999, hunk_index: 0 }),
  ];
  const h = { ...hunk, threads: [baseThread, resolvedThread, staleThread, fallbackThread, ownedThread] };
  return {
    ai: { threads: [baseThread, resolvedThread, staleThread, ownedThread], findings },
    files: [mkFile(FILE, [h])],
  };
}

// ---- Tests ----

describe("buildAnnotationIndex", () => {
  it("populates all index maps", () => {
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);

    expect(idx.findingsByFileLine.get(`${FILE}:13`)?.length).toBe(2); // f-line-13, f-owned
    expect(idx.findingsByFileLine.get(`${FILE}:12`)?.length).toBe(1);
    expect(idx.findingsByFile.get(FILE)?.length).toBe(1); // f-hunk

    expect(idx.threadMap.get("t1")).toBeDefined();
    expect(idx.threadMap.get("tOwned")).toBeDefined();

    expect(idx.findingThreadIds.has("tOwned")).toBe(true);
    expect(idx.findingThreadIds.has("t1")).toBe(false);

    expect(idx.threadsByHunk.get(`${FILE}#0`)?.length).toBe(5);
  });
});

describe("findingsForLine", () => {
  it("returns line-anchored findings", () => {
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);
    const out = findingsForLine(idx, FILE, 0, 13, hunkLines, false, "branch");
    expect(out.map((f) => f.id).sort()).toEqual(["f-line-13", "f-owned"]);
  });

  it("skipDelDuplicate drops del-row findings when a matching new_num exists", () => {
    // line 13 is added (kind=add). With skipDelDuplicate=true and hunk having a
    // new_num=13 line, the candidates should be filtered out.
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);
    const out = findingsForLine(idx, FILE, 0, 13, hunkLines, true, "branch");
    expect(out.length).toBe(0);
  });
});

describe("findingRendersInline", () => {
  it("true when finding's line appears in hunkLines (and not suppressed)", () => {
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);
    const f = ai.findings.find((x) => x.id === "f-line-13")!;
    expect(findingRendersInline(f, FILE, 0, hunkLines, "branch")).toBe(true);
  });

  it("false in branch mode when hunk_index mismatches", () => {
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);
    const f = ai.findings.find((x) => x.id === "f-line-13")!;
    expect(findingRendersInline(f, FILE, 99, hunkLines, "branch")).toBe(false);
  });

  it("del-only finding (line not in new_num set) still renders inline at the del row", () => {
    // Line 10 appears as del (old_num=10, kind=del). Since no add line has new_num=10,
    // the suppression branch doesn't fire, so it renders.
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);
    const f = ai.findings.find((x) => x.id === "f-del-only")!;
    expect(findingRendersInline(f, FILE, 0, hunkLines, "branch")).toBe(true);
  });
});

describe("findingBelongsToHunk", () => {
  it("branch mode: requires matching hunk_index", () => {
    const f = mkFinding({ id: "x", file: FILE, hunk_index: 0 });
    expect(findingBelongsToHunk(f, FILE, 0, hunk, "branch")).toBe(true);
    expect(findingBelongsToHunk(f, FILE, 1, hunk, "branch")).toBe(false);
  });

  it("non-branch mode: uses line range when hunk_index null", () => {
    const f = mkFinding({ id: "y", file: FILE, hunk_index: null, line: 12 });
    expect(findingBelongsToHunk(f, FILE, 0, hunk, "unstaged")).toBe(true);
    const fOut = mkFinding({ id: "z", file: FILE, hunk_index: null, line: 99 });
    expect(findingBelongsToHunk(fOut, FILE, 0, hunk, "unstaged")).toBe(false);
  });
});

describe("hunkLevelFindings", () => {
  it("returns no-line findings owned by hunk", () => {
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);
    const out = hunkLevelFindings(idx, FILE, 0, hunk, "branch");
    expect(out.map((f) => f.id)).toEqual(["f-hunk"]);
  });
});

describe("fallbackFindings", () => {
  it("returns line-anchored findings whose line is NOT rendered (e.g. outside hunk range)", () => {
    // We need a finding inside the new_start..new_start+new_count range but not rendered.
    // Build a small hunk that omits new_num=12 in lines.
    const sparseLines: LineSnapshot[] = [
      mkLine({ kind: "context", old_num: 9, new_num: 10, text: "" }),
      mkLine({ kind: "context", old_num: 12, new_num: 13, text: "" }),
    ];
    const sparseHunk = mkHunk({ new_start: 10, new_count: 4, lines: sparseLines });
    const findings: FlatFinding[] = [
      mkFinding({ id: "fb", file: FILE, line: 12, hunk_index: 0 }),
    ];
    const files: FileSnapshot[] = [mkFile(FILE, [sparseHunk])];
    const idx = buildAnnotationIndex({ threads: [], findings }, files, "branch", VIS_OFF);
    const out = fallbackFindings(idx, FILE, 0, sparseHunk, sparseLines, "branch");
    expect(out.map((f) => f.id)).toEqual(["fb"]);
  });
});

describe("findingsForSplitRow", () => {
  it("dedupes a finding that matches both left and right line numbers", () => {
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);
    const out = findingsForSplitRow(idx, FILE, 0, 13, 13, hunkLines, "branch");
    const ids = out.map((f) => f.id);
    expect(ids).toEqual(["f-line-13", "f-owned"]); // each appears once
  });
});

describe("threadsForLine", () => {
  it("filters out finding-owned threads and applies hideResolved/hideOutdated/hideAll", () => {
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);

    const all = threadsForLine(idx, FILE, 0, 13, hunkLines, { hideAll: false, hideResolved: false, hideOutdated: false });
    // baseThread (t1), resolvedThread (t2), staleThread (t3); tOwned excluded.
    expect(all.map((t) => t.id).sort()).toEqual(["t1", "t2", "t3"]);

    const hideResolved = threadsForLine(idx, FILE, 0, 13, hunkLines, { hideAll: false, hideResolved: true, hideOutdated: false });
    expect(hideResolved.map((t) => t.id).sort()).toEqual(["t1", "t3"]);

    const hideOutdated = threadsForLine(idx, FILE, 0, 13, hunkLines, { hideAll: false, hideResolved: false, hideOutdated: true });
    expect(hideOutdated.map((t) => t.id).sort()).toEqual(["t1", "t2"]);

    const hideAll = threadsForLine(idx, FILE, 0, 13, hunkLines, { hideAll: true, hideResolved: false, hideOutdated: false });
    expect(hideAll.length).toBe(0);
  });
});

describe("fallbackThreadsForHunk", () => {
  it("returns visible threads not anchored to any rendered line", () => {
    const { ai, files } = buildFixture();
    const idx = buildAnnotationIndex(ai, files, "branch", VIS_OFF);
    const rendered = new Set<number>([13]); // t1/t2/t3 match line 13
    const out = fallbackThreadsForHunk(idx, FILE, 0, hunk, rendered, VIS_OFF);
    expect(out.map((t) => t.id)).toEqual(["t4"]);
  });
});

describe("annotationVersion", () => {
  function fix() {
    return buildFixture();
  }

  it("is stable for identical inputs", () => {
    const a = fix();
    const v1 = annotationVersion(a.ai, a.files, "branch", VIS_OFF);
    const v2 = annotationVersion(a.ai, a.files, "branch", VIS_OFF);
    expect(v1).toBe(v2);
  });

  it("changes when ai.threads gains a thread", () => {
    const a = fix();
    const v1 = annotationVersion(a.ai, a.files, "branch", VIS_OFF);
    const ai2 = { ...a.ai, threads: [...a.ai.threads, mkThread({ id: "new", file: FILE, line: 10 })] };
    const v2 = annotationVersion(ai2, a.files, "branch", VIS_OFF);
    expect(v1).not.toBe(v2);
  });

  it("changes when a thread's resolved flips", () => {
    const a = fix();
    const v1 = annotationVersion(a.ai, a.files, "branch", VIS_OFF);
    const flipped = a.ai.threads.map((t) => t.id === "t1" ? { ...t, resolved: true } : t);
    const v2 = annotationVersion({ ...a.ai, threads: flipped }, a.files, "branch", VIS_OFF);
    expect(v1).not.toBe(v2);
  });

  it("changes when only hunk.threads gains a thread (no ai.threads delta)", () => {
    const a = fix();
    const v1 = annotationVersion(a.ai, a.files, "branch", VIS_OFF);
    const file = a.files[0];
    const h0 = file.hunks[0];
    const updated: FileSnapshot = { ...file, hunks: [{ ...h0, threads: [...h0.threads, mkThread({ id: "perHunkOnly", file: FILE, line: 11 })] }] };
    const v2 = annotationVersion(a.ai, [updated], "branch", VIS_OFF);
    expect(v1).not.toBe(v2);
  });

  it("changes when findings change", () => {
    const a = fix();
    const v1 = annotationVersion(a.ai, a.files, "branch", VIS_OFF);
    const v2 = annotationVersion({ ...a.ai, findings: [...a.ai.findings, mkFinding({ id: "newFinding", file: FILE })] }, a.files, "branch", VIS_OFF);
    expect(v1).not.toBe(v2);
  });

  it("changes when visibility bits flip", () => {
    const a = fix();
    const v1 = annotationVersion(a.ai, a.files, "branch", VIS_OFF);
    const v2 = annotationVersion(a.ai, a.files, "branch", { ...VIS_OFF, hideResolved: true });
    expect(v1).not.toBe(v2);
  });

  it("changes when mode changes", () => {
    const a = fix();
    const v1 = annotationVersion(a.ai, a.files, "branch", VIS_OFF);
    const v2 = annotationVersion(a.ai, a.files, "unstaged", VIS_OFF);
    expect(v1).not.toBe(v2);
  });
});
