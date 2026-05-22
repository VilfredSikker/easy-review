import { afterEach, beforeEach, describe, expect, it, mock } from "bun:test";
import { diffNav, type DiffNavigator } from "./diffNav.svelte";
import type { CrossFileModel } from "$lib/diffRenderModel";
import type { FileSnapshot } from "$lib/types";

function makeFile(path: string, overrides: Partial<FileSnapshot> = {}): FileSnapshot {
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
    hunks: [],
    source_index: 0,
    cache_key: "k",
    ...overrides,
  };
}

function makeModel(overrides: Partial<CrossFileModel> = {}): CrossFileModel {
  return {
    identity: "test",
    rows: [],
    cumulativeOffsets: [0],
    totalHeight: 0,
    fileStartRow: new Map(),
    rowFile: new Uint32Array(0),
    threadRowIndex: () => null,
    findingRowIndex: () => null,
    unifiedPairsByFile: new Map(),
    splitRowsByFile: new Map(),
    ...overrides,
  };
}

interface DocStub {
  getElementById: ReturnType<typeof mock>;
}

let origDocument: unknown;

beforeEach(() => {
  origDocument = (globalThis as { document?: unknown }).document;
  diffNav.unregister();
});

afterEach(() => {
  if (origDocument === undefined) {
    delete (globalThis as { document?: unknown }).document;
  } else {
    (globalThis as { document?: unknown }).document = origDocument;
  }
  diffNav.unregister();
});

function installDocStub(): DocStub {
  const scrollIntoView = mock(() => {});
  const el = {
    scrollIntoView,
    classList: { add: () => {}, remove: () => {} },
    offsetWidth: 0,
  };
  const getElementById = mock(() => el);
  (globalThis as { document: unknown }).document = { getElementById };
  return { getElementById };
}

describe("diffNav store", () => {
  it("scrollToFile with no registered navigator is a silent no-op", async () => {
    const doc = installDocStub();
    await expect(diffNav.scrollToFile("a.ts")).resolves.toBeUndefined();
    expect(doc.getElementById).not.toHaveBeenCalled();
  });

  it("legacy mode (getModel() === null) falls back to document.getElementById", async () => {
    const doc = installDocStub();
    const nav: DiffNavigator = {
      scrollToRow: mock(() => {}),
      requestFileContent: mock(async () => {}),
      getModel: () => null,
      getFiles: () => [makeFile("foo.ts")],
    };
    diffNav.register(nav);
    await diffNav.scrollToFile("foo.ts");
    expect(doc.getElementById).toHaveBeenCalledTimes(1);
    expect(doc.getElementById.mock.calls[0][0]).toBe("file-foo.ts");
    expect((nav.scrollToRow as ReturnType<typeof mock>)).not.toHaveBeenCalled();
  });

  it("flat mode resolves fileStartRow and calls scrollToRow(idx, 'start')", async () => {
    installDocStub();
    const scrollToRow = mock(() => {});
    const model = makeModel({
      fileStartRow: new Map([["foo.ts", 5]]),
    });
    diffNav.register({
      scrollToRow,
      requestFileContent: async () => {},
      getModel: () => model,
      getFiles: () => [makeFile("foo.ts")],
    });
    await diffNav.scrollToFile("foo.ts");
    expect(scrollToRow).toHaveBeenCalledTimes(1);
    expect(scrollToRow.mock.calls[0][0]).toBe(5);
    expect(scrollToRow.mock.calls[0][1]).toBe("start");
  });

  it("scrollToFile triggers requestFileContent for a lazy-stub target", async () => {
    installDocStub();
    const requestFileContent = mock(async () => {});
    const model = makeModel({ fileStartRow: new Map([["lazy.ts", 0]]) });
    diffNav.register({
      scrollToRow: () => {},
      requestFileContent,
      getModel: () => model,
      getFiles: () => [makeFile("lazy.ts", { is_lazy_stub: true, source_index: 7 })],
    });
    await diffNav.scrollToFile("lazy.ts");
    expect(requestFileContent).toHaveBeenCalledTimes(1);
    expect(requestFileContent.mock.calls[0][0]).toBe(7);
  });

  it("scrollToThread resolves threadRowIndex and calls scrollToRow(idx, 'center')", async () => {
    installDocStub();
    const scrollToRow = mock(() => {});
    const model = makeModel({
      threadRowIndex: (id) => (id === "t1" ? 12 : null),
    });
    diffNav.register({
      scrollToRow,
      requestFileContent: async () => {},
      getModel: () => model,
      getFiles: () => [],
    });
    await diffNav.scrollToThread("t1");
    expect(scrollToRow).toHaveBeenCalledTimes(1);
    expect(scrollToRow.mock.calls[0][0]).toBe(12);
    expect(scrollToRow.mock.calls[0][1]).toBe("center");
  });

  it("scrollToThread falls back to DOM when threadRowIndex returns null", async () => {
    const doc = installDocStub();
    const scrollToRow = mock(() => {});
    const model = makeModel({ threadRowIndex: () => null });
    diffNav.register({
      scrollToRow,
      requestFileContent: async () => {},
      getModel: () => model,
      getFiles: () => [],
    });
    await diffNav.scrollToThread("missing");
    expect(scrollToRow).not.toHaveBeenCalled();
    // getElementById called for scroll + flash (same id).
    expect(doc.getElementById).toHaveBeenCalled();
    expect(doc.getElementById.mock.calls[0][0]).toBe("missing");
  });

  it("unregister returns the store to the silent no-op state", async () => {
    const doc = installDocStub();
    diffNav.register({
      scrollToRow: () => {},
      requestFileContent: async () => {},
      getModel: () => null,
      getFiles: () => [],
    });
    diffNav.unregister();
    await diffNav.scrollToFile("a");
    expect(doc.getElementById).not.toHaveBeenCalled();
  });
});
