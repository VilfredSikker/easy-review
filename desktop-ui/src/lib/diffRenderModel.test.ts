import { describe, expect, it } from "vitest";
import {
  COMPACTED_STUB_HEIGHT,
  FILE_HEADER_HEIGHT,
  HUNK_HEADER_HEIGHT,
  LINE_HEIGHT,
  NO_CHANGES_HEIGHT,
  estimateLazyStubHeight,
  diffLineCount,
  filesRenderFingerprint,
  getFileBlock,
  getCrossFileModel,
  type CrossFileFlatRow,
  type RenderModelInputs,
} from "./diffRenderModel";
import {
  buildAnnotationIndex,
  type CommentVisibility,
} from "./diffAnnotations";
import type {
  AiSnapshot,
  FileSnapshot,
  FlatFinding,
  HunkSnapshot,
  LineSnapshot,
  ThreadSnapshot,
} from "./types";

// ---------------- Fixture builders ----------------

function line(opts: Partial<LineSnapshot> & { kind: LineSnapshot["kind"] }): LineSnapshot {
  return {
    old_num: opts.old_num ?? null,
    new_num: opts.new_num ?? null,
    kind: opts.kind,
    text: opts.text ?? "",
    spans: opts.spans,
  };
}

function hunk(opts: {
  header?: string;
  old_start?: number;
  old_count?: number;
  new_start?: number;
  new_count?: number;
  lines: LineSnapshot[];
  threads?: ThreadSnapshot[];
}): HunkSnapshot {
  return {
    header: opts.header ?? "@@ -1,1 +1,1 @@",
    old_start: opts.old_start ?? 1,
    old_count: opts.old_count ?? 1,
    new_start: opts.new_start ?? 1,
    new_count: opts.new_count ?? 1,
    lines: opts.lines,
    threads: opts.threads ?? [],
  };
}

function file(opts: Partial<FileSnapshot> & { path: string; hunks: HunkSnapshot[] }): FileSnapshot {
  return {
    path: opts.path,
    status: opts.status ?? "modified",
    additions: opts.additions ?? 0,
    deletions: opts.deletions ?? 0,
    reviewed: opts.reviewed ?? false,
    compacted: opts.compacted ?? false,
    risk: opts.risk ?? null,
    finding_count: opts.finding_count ?? 0,
    comment_count: opts.comment_count ?? 0,
    question_count: opts.question_count ?? 0,
    hunks: opts.hunks,
    is_lazy_stub: opts.is_lazy_stub,
    source_index: opts.source_index ?? 0,
    cache_key: opts.cache_key ?? `${opts.path}#0`,
  };
}

function thread(id: string, fileP: string, ln: number, extra: Partial<ThreadSnapshot> = {}): ThreadSnapshot {
  return {
    id,
    kind: "comment",
    file: fileP,
    line: ln,
    source: "local",
    synced: false,
    stale: false,
    resolved: false,
    root: { id: `${id}-root`, author: "me", kind: "you", timestamp: "", body_markdown: "" },
    replies: [],
    promoted_to: null,
    ...extra,
  };
}

function finding(id: string, fileP: string, ln: number | null, extra: Partial<FlatFinding> = {}): FlatFinding {
  return {
    id,
    file: fileP,
    line: ln,
    hunk_index: null,
    severity: "low",
    title: "",
    message_markdown: "",
    promoted_to: null,
    thread_id: null,
    ...extra,
  };
}

function emptyAi(threads: ThreadSnapshot[] = [], findings: FlatFinding[] = []): AiSnapshot {
  return {
    fresh: true,
    stale_reason: null,
    summary_markdown: null,
    high: 0,
    med: 0,
    low: 0,
    local_comment_count: 0,
    github_comment_count: 0,
    comments: 0,
    questions: 0,
    unpushed: 0,
    threads,
    findings,
  };
}

const VIS_DEFAULT: CommentVisibility = {
  hideAll: false,
  hideResolved: false,
  hideOutdated: false,
};

function mkInputs(
  f: FileSnapshot,
  files: FileSnapshot[],
  ai: AiSnapshot,
  viewMode: "unified" | "split" = "unified",
  vis: CommentVisibility = VIS_DEFAULT,
  mode: string = "branch",
  fileIndex: number = 0,
): RenderModelInputs {
  return {
    file: f,
    fileIndex,
    viewMode,
    mode,
    annotationIndex: buildAnnotationIndex(ai, files, mode, vis),
    commentVisibility: vis,
  };
}

// ---------------- Legacy tests retained-by-implication via reusing getFileRenderModel elsewhere ----------------

describe("getFileBlock — unified mode row enumeration", () => {
  it("emits file-header → hunk-header → content rows × N for each hunk", () => {
    const h1 = hunk({
      header: "@@ -1,3 +1,3 @@",
      lines: [
        line({ kind: "context", old_num: 1, new_num: 1, text: "a" }),
        line({ kind: "context", old_num: 2, new_num: 2, text: "b" }),
        line({ kind: "context", old_num: 3, new_num: 3, text: "c" }),
      ],
    });
    const h2 = hunk({
      header: "@@ -10,2 +10,2 @@",
      old_start: 10,
      old_count: 2,
      new_start: 10,
      new_count: 2,
      lines: [
        line({ kind: "context", old_num: 10, new_num: 10, text: "x" }),
        line({ kind: "context", old_num: 11, new_num: 11, text: "y" }),
      ],
    });
    const f = file({ path: "a.ts", hunks: [h1, h2] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi()));

    // file-header + (hunk-header + 3 content) + (hunk-header + 2 content) = 1 + 4 + 3 = 8
    expect(block.rows.length).toBe(8);
    expect(block.rows[0].type).toBe("file-header");
    expect(block.rows[1].type).toBe("hunk-header");
    expect(block.rows[2].type).toBe("content-unified");
    expect(block.rows[3].type).toBe("content-unified");
    expect(block.rows[4].type).toBe("content-unified");
    expect(block.rows[5].type).toBe("hunk-header");
    expect(block.rows[6].type).toBe("content-unified");
    expect(block.rows[7].type).toBe("content-unified");
  });
});

describe("getFileBlock — split mode", () => {
  it("emits content-split rows count matching splitRows length", () => {
    const h1 = hunk({
      lines: [
        line({ kind: "del", old_num: 1, text: "a" }),
        line({ kind: "add", new_num: 1, text: "A" }),
        line({ kind: "context", old_num: 2, new_num: 2, text: "b" }),
      ],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi(), "split"));
    const splitRowCount = block.splitRowsByHunk[0].length;
    const contentSplitRows = block.rows.filter((r) => r.type === "content-split");
    expect(contentSplitRows.length).toBe(splitRowCount);
  });
});

describe("getFileBlock — bypass cases", () => {
  it("lazy-stub file emits exactly file-header + lazy-stub", () => {
    const f = file({
      path: "big.ts",
      hunks: [],
      is_lazy_stub: true,
      additions: 50,
      deletions: 20,
    });
    const block = getFileBlock(mkInputs(f, [f], emptyAi()));
    expect(block.rows.length).toBe(2);
    expect(block.rows[0].type).toBe("file-header");
    expect(block.rows[1].type).toBe("lazy-stub");
    const expectedStub = estimateLazyStubHeight(f);
    expect(block.totalHeight).toBe(FILE_HEADER_HEIGHT + expectedStub);
  });

  it("compacted file emits exactly file-header + compacted-stub", () => {
    const f = file({ path: "p.lock", hunks: [], compacted: true });
    const block = getFileBlock(mkInputs(f, [f], emptyAi()));
    expect(block.rows.length).toBe(2);
    expect(block.rows[1].type).toBe("compacted-stub");
    expect(block.totalHeight).toBe(FILE_HEADER_HEIGHT + COMPACTED_STUB_HEIGHT);
  });

  it("no-changes file emits exactly file-header + no-changes", () => {
    const f = file({ path: "empty.ts", hunks: [] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi()));
    expect(block.rows.length).toBe(2);
    expect(block.rows[1].type).toBe("no-changes");
    expect(block.totalHeight).toBe(FILE_HEADER_HEIGHT + NO_CHANGES_HEIGHT);
  });
});

describe("getFileBlock — thread/finding injection", () => {
  it("inline-thread appears immediately after content row for anchor line", () => {
    const t = thread("t1", "a.ts", 2);
    const h1 = hunk({
      header: "@@ -1,3 +1,3 @@",
      old_start: 1,
      old_count: 3,
      new_start: 1,
      new_count: 3,
      lines: [
        line({ kind: "context", old_num: 1, new_num: 1 }),
        line({ kind: "context", old_num: 2, new_num: 2 }),
        line({ kind: "context", old_num: 3, new_num: 3 }),
      ],
      threads: [t],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi([t])));
    // file-header, hunk-header, content(1), content(2), inline-thread, content(3)
    expect(block.rows[3].type).toBe("content-unified");
    expect(block.rows[4].type).toBe("inline-thread");
    if (block.rows[4].type === "inline-thread") {
      expect(block.rows[4].identity).toBe("it:t1");
      expect(block.rows[4].threadId).toBe("t1");
    }
    expect(block.rows[5].type).toBe("content-unified");
  });

  it("inline-finding appears immediately after content row for anchor line", () => {
    const fnd = finding("f1", "a.ts", 2);
    const h1 = hunk({
      old_start: 1,
      old_count: 3,
      new_start: 1,
      new_count: 3,
      lines: [
        line({ kind: "context", old_num: 1, new_num: 1 }),
        line({ kind: "context", old_num: 2, new_num: 2 }),
        line({ kind: "context", old_num: 3, new_num: 3 }),
      ],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi([], [fnd])));
    expect(block.rows[3].type).toBe("content-unified");
    expect(block.rows[4].type).toBe("inline-finding");
    if (block.rows[4].type === "inline-finding") {
      expect(block.rows[4].findingId).toBe("f1");
    }
  });

  it("hunk-level finding (no line anchor) appears as fallback-finding after content rows", () => {
    const fnd = finding("f1", "a.ts", null, { hunk_index: 0 });
    const h1 = hunk({
      old_start: 1,
      old_count: 2,
      new_start: 1,
      new_count: 2,
      lines: [
        line({ kind: "context", old_num: 1, new_num: 1 }),
        line({ kind: "context", old_num: 2, new_num: 2 }),
      ],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi([], [fnd])));
    // file-header, hunk-header, c1, c2, fallback-finding
    expect(block.rows[block.rows.length - 1].type).toBe("fallback-finding");
  });

  it("fallback thread (line not rendered) appears as fallback-thread at hunk footer", () => {
    const t = thread("t1", "a.ts", 99); // line 99 not in hunk
    const h1 = hunk({
      old_start: 1,
      old_count: 2,
      new_start: 1,
      new_count: 2,
      lines: [
        line({ kind: "context", old_num: 1, new_num: 1 }),
        line({ kind: "context", old_num: 2, new_num: 2 }),
      ],
      threads: [t],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi([t])));
    const last = block.rows[block.rows.length - 1];
    expect(last.type).toBe("fallback-thread");
    if (last.type === "fallback-thread") {
      expect(last.threadId).toBe("t1");
    }
  });
});

describe("getFileBlock — geometry & invariants", () => {
  it("sum of heights equals totalHeight; cumulativeOffsets length = rows + 1", () => {
    const h1 = hunk({
      lines: [
        line({ kind: "context", old_num: 1, new_num: 1 }),
        line({ kind: "context", old_num: 2, new_num: 2 }),
      ],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi()));
    const sum = block.rows.reduce((acc, r) => acc + r.height, 0);
    expect(sum).toBe(block.totalHeight);
    expect(block.cumulativeOffsets.length).toBe(block.rows.length + 1);
    expect(block.cumulativeOffsets[block.rows.length]).toBe(block.totalHeight);
    expect(block.cumulativeOffsets[0]).toBe(0);
  });

  it("file-header is row 0", () => {
    const f = file({ path: "a.ts", hunks: [] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi()));
    expect(block.rows[0].type).toBe("file-header");
  });

  it("identities are unique within the block", () => {
    const t = thread("t1", "a.ts", 1);
    const fnd = finding("f1", "a.ts", 2);
    const h1 = hunk({
      old_start: 1,
      old_count: 2,
      new_start: 1,
      new_count: 2,
      lines: [
        line({ kind: "context", old_num: 1, new_num: 1 }),
        line({ kind: "context", old_num: 2, new_num: 2 }),
      ],
      threads: [t],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const block = getFileBlock(mkInputs(f, [f], emptyAi([t], [fnd])));
    const ids = block.rows.map((r) => r.identity);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("getFileBlock — caching", () => {
  it("returns same object on repeat call with same inputs", () => {
    const h1 = hunk({
      lines: [line({ kind: "context", old_num: 1, new_num: 1 })],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const inputs = mkInputs(f, [f], emptyAi());
    const a = getFileBlock(inputs);
    const b = getFileBlock(inputs);
    expect(a).toBe(b);
  });

  it("unified vs split produce different blocks for same file", () => {
    const h1 = hunk({
      lines: [
        line({ kind: "del", old_num: 1, text: "a" }),
        line({ kind: "add", new_num: 1, text: "A" }),
      ],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const u = getFileBlock(mkInputs(f, [f], emptyAi(), "unified"));
    const s = getFileBlock(mkInputs(f, [f], emptyAi(), "split"));
    expect(u).not.toBe(s);
    expect(u.modelKey).not.toBe(s.modelKey);
  });

  it("busts cache when cache_key or hunk lines change on same file object", () => {
    const f = file({
      path: "a.ts",
      cache_key: "k1",
      hunks: [hunk({ lines: [line({ kind: "context", old_num: 1, new_num: 1 })] })],
    });
    const a = getFileBlock(mkInputs(f, [f], emptyAi()));
    f.cache_key = "k2";
    f.hunks = [
      hunk({
        lines: [
          line({ kind: "context", old_num: 1, new_num: 1 }),
          line({ kind: "add", new_num: 2, text: "x" }),
        ],
      }),
    ];
    const b = getFileBlock(mkInputs(f, [f], emptyAi()));
    expect(a).not.toBe(b);
    expect(b.rows.length).toBeGreaterThan(a.rows.length);
  });

  it("changing commentVisibility busts the cache", () => {
    const h1 = hunk({
      lines: [line({ kind: "context", old_num: 1, new_num: 1 })],
    });
    const f = file({ path: "a.ts", hunks: [h1] });
    const a = getFileBlock(mkInputs(f, [f], emptyAi(), "unified", VIS_DEFAULT));
    const visHide: CommentVisibility = { hideAll: true, hideResolved: false, hideOutdated: false };
    const b = getFileBlock(mkInputs(f, [f], emptyAi(), "unified", visHide));
    expect(a).not.toBe(b);
    expect(a.modelKey).not.toBe(b.modelKey);
  });
});

// ---------------- Step B: cross-file model tests ----------------

import { buildAnnotationIndex as _build } from "./diffAnnotations";
import type { AiSnapshot, FileSnapshot as _FS } from "./types";

function mkCross(
  files: FileSnapshot[],
  ai: AiSnapshot,
  opts: {
    viewMode?: "unified" | "split";
    vis?: CommentVisibility;
    mode?: string;
    snapshotKey?: string;
  } = {},
) {
  const viewMode = opts.viewMode ?? "unified";
  const vis = opts.vis ?? VIS_DEFAULT;
  const mode = opts.mode ?? "branch";
  const snapshotKey = opts.snapshotKey ?? "tab:branch:main:feat";
  return getCrossFileModel({
    files,
    viewMode,
    mode,
    annotationIndex: _build(ai, files, mode, vis),
    commentVisibility: vis,
    snapshotKey,
  });
}

function makeSimpleFile(path: string, ctxLines = 2): FileSnapshot {
  const lines: LineSnapshot[] = [];
  for (let i = 1; i <= ctxLines; i++) {
    lines.push(line({ kind: "context", old_num: i, new_num: i, text: `${path}-${i}` }));
  }
  return file({ path, hunks: [hunk({ lines })] });
}

describe("getCrossFileModel — concatenation & layout", () => {
  it("rows = file0 ++ file1 ++ file2 in order", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const f1 = makeSimpleFile("b.ts", 2);
    const f2 = makeSimpleFile("c.ts", 3);
    const ai = emptyAi();
    const m = mkCross([f0, f1, f2], ai, { snapshotKey: "s1" });
    const b0 = getFileBlock(mkInputs(f0, [f0, f1, f2], ai, "unified", VIS_DEFAULT, "branch", 0));
    const b1 = getFileBlock(mkInputs(f1, [f0, f1, f2], ai, "unified", VIS_DEFAULT, "branch", 1));
    const b2 = getFileBlock(mkInputs(f2, [f0, f1, f2], ai, "unified", VIS_DEFAULT, "branch", 2));
    expect(m.rows.length).toBe(b0.rows.length + b1.rows.length + b2.rows.length);
    expect(m.rows[0]).toBe(b0.rows[0]);
    expect(m.rows[b0.rows.length]).toBe(b1.rows[0]);
    expect(m.rows[b0.rows.length + b1.rows.length]).toBe(b2.rows[0]);
  });

  it("fileStartRow points at file-header for each file", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const f1 = makeSimpleFile("b.ts", 2);
    const m = mkCross([f0, f1], emptyAi(), { snapshotKey: "s2" });
    const s0 = m.fileStartRow.get("a.ts")!;
    const s1 = m.fileStartRow.get("b.ts")!;
    expect(m.rows[s0].type).toBe("file-header");
    expect(m.rows[s1].type).toBe("file-header");
    if (m.rows[s0].type === "file-header") expect(m.rows[s0].filePath).toBe("a.ts");
    if (m.rows[s1].type === "file-header") expect(m.rows[s1].filePath).toBe("b.ts");
  });

  it("rowFile[i] is the file index for each row", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const f1 = makeSimpleFile("b.ts", 2);
    const m = mkCross([f0, f1], emptyAi(), { snapshotKey: "s3" });
    const start1 = m.fileStartRow.get("b.ts")!;
    for (let i = 0; i < start1; i++) expect(m.rowFile[i]).toBe(0);
    for (let i = start1; i < m.rows.length; i++) expect(m.rowFile[i]).toBe(1);
  });

  it("totalHeight equals sum of per-file block totals; cumulativeOffsets length = rows + 1", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const f1 = makeSimpleFile("b.ts", 3);
    const ai = emptyAi();
    const m = mkCross([f0, f1], ai, { snapshotKey: "s4" });
    const b0 = getFileBlock(mkInputs(f0, [f0, f1], ai, "unified", VIS_DEFAULT, "branch", 0));
    const b1 = getFileBlock(mkInputs(f1, [f0, f1], ai, "unified", VIS_DEFAULT, "branch", 1));
    expect(m.totalHeight).toBe(b0.totalHeight + b1.totalHeight);
    expect(m.cumulativeOffsets.length).toBe(m.rows.length + 1);
    expect(m.cumulativeOffsets[m.rows.length]).toBe(m.totalHeight);
  });

  it("empty files list returns empty model with cumulativeOffsets=[0]", () => {
    const m = mkCross([], emptyAi(), { snapshotKey: "empty" });
    expect(m.rows.length).toBe(0);
    expect(m.totalHeight).toBe(0);
    expect(m.cumulativeOffsets).toEqual([0]);
    expect(m.threadRowIndex("x")).toBeNull();
    expect(m.findingRowIndex("x")).toBeNull();
  });
});

describe("getCrossFileModel — identity & cache invalidation", () => {
  it("viewMode swap produces a different model object", () => {
    const f0 = file({
      path: "a.ts",
      hunks: [hunk({ lines: [line({ kind: "del", old_num: 1 }), line({ kind: "add", new_num: 1 })] })],
    });
    const ai = emptyAi();
    const u = mkCross([f0], ai, { viewMode: "unified", snapshotKey: "vm" });
    const s = mkCross([f0], ai, { viewMode: "split", snapshotKey: "vm" });
    expect(u).not.toBe(s);
  });

  it("adding a thread to ai.threads busts identity", () => {
    const f0 = makeSimpleFile("a.ts", 2);
    const a = mkCross([f0], emptyAi(), { snapshotKey: "av" });
    const t = thread("t-new", "a.ts", 1);
    const b = mkCross([f0], emptyAi([t]), { snapshotKey: "av" });
    expect(a).not.toBe(b);
    expect(a.identity).not.toBe(b.identity);
  });

  it("adding a thread to hunk.threads (not ai.threads) busts identity", () => {
    const f0 = makeSimpleFile("a.ts", 2);
    const a = mkCross([f0], emptyAi(), { snapshotKey: "hv" });
    const t = thread("hk-t", "a.ts", 1);
    const f0b = file({ path: "a.ts", hunks: [hunk({ lines: f0.hunks[0].lines, threads: [t] })] });
    const b = mkCross([f0b], emptyAi(), { snapshotKey: "hv" });
    expect(a.identity).not.toBe(b.identity);
  });

  it("toggling hideResolved changes identity", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const a = mkCross([f0], emptyAi(), { snapshotKey: "vis" });
    const b = mkCross([f0], emptyAi(), {
      snapshotKey: "vis",
      vis: { hideAll: false, hideResolved: true, hideOutdated: false },
    });
    expect(a.identity).not.toBe(b.identity);
  });

  it("cache hit returns same object for identical inputs", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const a = mkCross([f0], emptyAi(), { snapshotKey: "hit" });
    const b = mkCross([f0], emptyAi(), { snapshotKey: "hit" });
    expect(a).toBe(b);
  });

  it("busts cross-file cache when file cache_key changes", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const a = mkCross([f0], emptyAi(), { snapshotKey: "same-tab" });
    f0.cache_key = "updated";
    f0.hunks = [
      hunk({
        lines: [
          line({ kind: "context", old_num: 1, new_num: 1 }),
          line({ kind: "add", new_num: 2, text: "new" }),
        ],
      }),
    ];
    const b = mkCross([f0], emptyAi(), { snapshotKey: "same-tab" });
    expect(a).not.toBe(b);
    expect(b.rows.length).toBeGreaterThan(a.rows.length);
  });
});

describe("filesRenderFingerprint", () => {
  it("changes when line count changes", () => {
    const f = makeSimpleFile("a.ts", 1);
    const fp1 = filesRenderFingerprint([f]);
    f.hunks[0].lines.push(line({ kind: "add", new_num: 2, text: "x" }));
    const fp2 = filesRenderFingerprint([f]);
    expect(fp1).not.toBe(fp2);
    expect(diffLineCount(f)).toBe(2);
  });
});

describe("getCrossFileModel — thread/finding lookups", () => {
  it("threadRowIndex returns row index for an inline thread; null for unknown", () => {
    const t = thread("t1", "a.ts", 1);
    const f0 = file({
      path: "a.ts",
      hunks: [hunk({ lines: [line({ kind: "context", old_num: 1, new_num: 1 })], threads: [t] })],
    });
    const m = mkCross([f0], emptyAi([t]), { snapshotKey: "tr" });
    const idx = m.threadRowIndex("t1");
    expect(idx).not.toBeNull();
    if (idx !== null) {
      const row = m.rows[idx];
      expect(row.type === "inline-thread" || row.type === "fallback-thread").toBe(true);
    }
    expect(m.threadRowIndex("nope")).toBeNull();
  });

  it("findingRowIndex returns row index for a finding; null for unknown", () => {
    const fnd = finding("f1", "a.ts", 1);
    const f0 = file({
      path: "a.ts",
      hunks: [hunk({ lines: [line({ kind: "context", old_num: 1, new_num: 1 })] })],
    });
    const m = mkCross([f0], emptyAi([], [fnd]), { snapshotKey: "fr" });
    const idx = m.findingRowIndex("f1");
    expect(idx).not.toBeNull();
    if (idx !== null) {
      const row = m.rows[idx];
      expect(row.type === "inline-finding" || row.type === "fallback-finding").toBe(true);
    }
    expect(m.findingRowIndex("nope")).toBeNull();
  });
});

describe("getCrossFileModel — LRU eviction", () => {
  it("evicts oldest entry past the size-4 limit", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const m1 = mkCross([f0], emptyAi(), { snapshotKey: "k1" });
    mkCross([f0], emptyAi(), { snapshotKey: "k2" });
    mkCross([f0], emptyAi(), { snapshotKey: "k3" });
    mkCross([f0], emptyAi(), { snapshotKey: "k4" });
    mkCross([f0], emptyAi(), { snapshotKey: "k5" }); // evicts k1
    const m1Again = mkCross([f0], emptyAi(), { snapshotKey: "k1" });
    expect(m1Again).not.toBe(m1);
  });

  it("recently-accessed entry is not evicted", () => {
    const f0 = makeSimpleFile("a.ts", 1);
    const a1 = mkCross([f0], emptyAi(), { snapshotKey: "r-a" });
    mkCross([f0], emptyAi(), { snapshotKey: "r-b" }); // B
    const a2 = mkCross([f0], emptyAi(), { snapshotKey: "r-a" }); // touch A → most recent
    expect(a2).toBe(a1);
    mkCross([f0], emptyAi(), { snapshotKey: "r-c" });
    mkCross([f0], emptyAi(), { snapshotKey: "r-d" });
    mkCross([f0], emptyAi(), { snapshotKey: "r-e" }); // evicts B (LRU)
    const a3 = mkCross([f0], emptyAi(), { snapshotKey: "r-a" });
    expect(a3).toBe(a1);
  });
});
