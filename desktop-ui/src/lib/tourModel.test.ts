import { describe, expect, it } from "bun:test";
import {
  activePillarAt,
  fileEntryRow,
  linkFileRefs,
  narrativeHtml,
  nextPillarId,
  pad2,
  pillarHeaderSpecs,
  pillarOrder,
  railPillars,
  resolveDiffPath,
} from "./tourModel";
import type { FileSnapshot, PillarSnapshot, TourFileRef } from "./types";

function file(path: string, reviewed = false): FileSnapshot {
  return { path, reviewed, additions: 1, deletions: 0, source_index: 0 } as FileSnapshot;
}

function pillar(id: string, refs: TourFileRef[], extra: Partial<PillarSnapshot> = {}): PillarSnapshot {
  return {
    id,
    title: `Title ${id}`,
    descriptionMarkdown: `About ${id}.`,
    importance: 50,
    foundation: false,
    files: refs,
    reviewedCount: 0,
    totalCount: refs.length,
    ...extra,
  };
}

const ref = (path: string, related: TourFileRef["related"] = []): TourFileRef => ({
  path,
  reason: "",
  findingIds: [],
  related,
});

describe("pillarHeaderSpecs", () => {
  it("anchors each pillar on its first owned file and numbers them", () => {
    const pillars = [pillar("p1", [ref("a.ts")]), pillar("p2", [ref("a.ts"), ref("b.ts")])];
    const specs = pillarHeaderSpecs(pillars, [file("a.ts"), file("b.ts")]);
    expect(specs.map((s) => [s.pillarId, s.index, s.total, s.anchorPath])).toEqual([
      ["p1", 1, 2, "a.ts"],
      ["p2", 2, 2, "b.ts"],
    ]);
    expect(specs[0].narrativeMarkdown).toBe("About p1.");
  });

  it("lets a pillar with no drawn file borrow the next anchor, or trail", () => {
    const pillars = [pillar("p1", [ref("gone.ts")]), pillar("p2", [ref("b.ts")]), pillar("p3", [])];
    const specs = pillarHeaderSpecs(pillars, [file("b.ts")]);
    expect(specs.map((s) => s.anchorPath)).toEqual(["b.ts", "b.ts", null]);
  });

  it("is empty without pillars", () => {
    expect(pillarHeaderSpecs([], [file("a.ts")])).toEqual([]);
  });
});

describe("activePillarAt", () => {
  const starts = new Map([
    ["p1", 0],
    ["p2", 3],
  ]);
  const offsets = [0, 100, 140, 200, 300, 340];

  it("picks the last header at or above the reading line", () => {
    expect(activePillarAt(starts, offsets, 150)).toBe("p1");
    expect(activePillarAt(starts, offsets, 200)).toBe("p2");
    expect(activePillarAt(starts, offsets, 9999)).toBe("p2");
  });

  it("falls back to the first pillar above every header, and null without any", () => {
    expect(activePillarAt(new Map([["p1", 2]]), offsets, 10)).toBe("p1");
    expect(activePillarAt(new Map(), offsets, 10)).toBeNull();
  });
});

describe("next pillar", () => {
  it("orders by row and stops on the last", () => {
    const order = pillarOrder(
      new Map([
        ["b", 9],
        ["a", 1],
      ]),
    );
    expect(order).toEqual(["a", "b"]);
    expect(nextPillarId(order, "a")).toBe("b");
    expect(nextPillarId(order, "b")).toBe("b");
    expect(nextPillarId(order, null)).toBe("a");
    expect(nextPillarId([], "a")).toBeNull();
  });
});

describe("railPillars", () => {
  it("nests related files under their primary and keeps snapshot counts", () => {
    const p = pillar("p1", [ref("a.ts", [{ path: "a.test.ts", kind: "test", reason: "" }])], {
      reviewedCount: 1,
      totalCount: 2,
      summaryMarkdown: "One line.",
    });
    const [rail] = railPillars([p], [file("a.ts"), file("a.test.ts", true)]);
    expect(rail.files.map((r) => [r.file.path, r.relatedKind])).toEqual([
      ["a.ts", null],
      ["a.test.ts", "test"],
    ]);
    expect([rail.reviewed, rail.total, rail.summary]).toEqual([1, 2, "One line."]);
  });

  it("lists a shared file under the first pillar only, and falls back to the description", () => {
    const rails = railPillars([pillar("p1", [ref("a.ts")]), pillar("p2", [ref("a.ts")])], [file("a.ts")]);
    expect(rails[1].files).toEqual([]);
    expect(rails[1].summary).toBe("About p2.");
  });
});

describe("resolveDiffPath", () => {
  const paths = ["db/schema.sql", "src/app.ts", "web/app.ts"];

  it("matches the full path, prefixes and line suffixes", () => {
    expect(resolveDiffPath("db/schema.sql", paths)).toBe("db/schema.sql");
    expect(resolveDiffPath("./db/schema.sql:12", paths)).toBe("db/schema.sql");
    expect(resolveDiffPath("/src/app.ts#L3-L9", paths)).toBe("src/app.ts");
  });

  it("matches a unique basename or suffix, never an ambiguous or outside one", () => {
    expect(resolveDiffPath("schema.sql", paths)).toBe("db/schema.sql");
    expect(resolveDiffPath("app.ts", paths)).toBeNull();
    expect(resolveDiffPath("web/app.ts", paths)).toBe("web/app.ts");
    expect(resolveDiffPath("other.ts", paths)).toBeNull();
    expect(resolveDiffPath("https://x.dev/schema.sql", paths)).toBeNull();
    expect(resolveDiffPath("db schema", paths)).toBeNull();
  });
});

describe("linkFileRefs", () => {
  const paths = ["db/schema.sql"];

  it("links markdown links and code spans that name a diff file", () => {
    expect(linkFileRefs("see [the schema](db/schema.sql) now", paths)).toBe(
      'see <a class="ref" href="#" data-goto="db/schema.sql">the schema</a> now',
    );
    expect(linkFileRefs("in <code>schema.sql</code>", paths)).toBe(
      'in <a class="ref" href="#" data-goto="db/schema.sql"><code>schema.sql</code></a>',
    );
  });

  it("leaves other code spans and links alone, and never nests anchors", () => {
    expect(linkFileRefs("<code>fooBar()</code> [x](nowhere.md)", paths)).toBe(
      "<code>fooBar()</code> [x](nowhere.md)",
    );
    expect(linkFileRefs("[<code>schema.sql</code>](db/schema.sql)", paths)).toBe(
      '<a class="ref" href="#" data-goto="db/schema.sql"><code>schema.sql</code></a>',
    );
  });
});

describe("narrativeHtml", () => {
  it("renders paragraphs and lists with linked references, escaping everything else", () => {
    const html = narrativeHtml("Start in `schema.sql` <b>now</b>.\n\n- one\n- two", ["db/schema.sql"]);
    expect(html).toBe(
      '<p>Start in <a class="ref" href="#" data-goto="db/schema.sql"><code>schema.sql</code></a> &lt;b&gt;now&lt;/b&gt;.</p>' +
        "<ul><li>one</li><li>two</li></ul>",
    );
  });

  it("escapes code blocks", () => {
    expect(narrativeHtml("```\n<script>\n```", [])).toBe("<pre><code>&lt;script&gt;</code></pre>");
  });
});

describe("pad2", () => {
  it("pads to two digits", () => {
    expect(pad2(3)).toBe("03");
    expect(pad2(12)).toBe("12");
  });
});

describe("fileEntryRow", () => {
  const rows = [
    { type: "pillar-header" },
    { type: "file-header" },
    { type: "diff-line" },
    { type: "file-header" },
    { type: "diff-line" },
    { type: "pillar-header" },
    { type: "file-header" },
  ];

  it("opens a pillar's first file at the pillar header", () => {
    expect(fileEntryRow(rows, 6)).toBe(5);
    expect(fileEntryRow(rows, 1)).toBe(0);
  });

  it("lands on the file header mid-pillar", () => {
    expect(fileEntryRow(rows, 3)).toBe(3);
  });

  it("leaves the Diff view, which has no pillar headers, unchanged", () => {
    expect(fileEntryRow([{ type: "file-header" }, { type: "diff-line" }], 0)).toBe(0);
  });
});
