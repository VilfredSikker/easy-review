import { describe, expect, it } from "bun:test";
import {
  DEFAULT_EXPORT_PREFS,
  buildExportOpts,
  collectExportItems,
  exportItemCount,
  exportTabKey,
  highlightMarkdown,
  itemLocation,
  parseExportPrefs,
  previewSize,
  visibleItems,
  type ExportPrefs,
} from "./exportOptions";
import type { AiSnapshot, AppSnapshot, FlatFinding, ThreadSnapshot, UiAnnotation } from "./types";

function thread(id: string, kind: ThreadSnapshot["kind"], resolved = false): ThreadSnapshot {
  return {
    id,
    kind,
    file: "src/a.ts",
    line: 3,
    source: "local",
    synced: false,
    stale: false,
    resolved,
    root: { body_markdown: `body ${id}\nsecond line` },
    replies: [],
    promoted_to: null,
  } as unknown as ThreadSnapshot;
}

function finding(id: string, extra: Partial<FlatFinding> = {}): FlatFinding {
  return { id, file: "src/b.ts", line: 7, title: `title ${id}`, resolved: false, confidence: "confirmed", ...extra } as FlatFinding;
}

function ai(over: Partial<AiSnapshot> = {}): AiSnapshot {
  return {
    threads: [thread("c1", "comment"), thread("q1", "question"), thread("q2", "question", true), thread("n1", "note")],
    findings: [finding("f1")],
    resolved_findings: [finding("f2", { resolved: true })],
    dropped_findings: [finding("f3", { confidence: "dropped" })],
    ...over,
  } as AiSnapshot;
}

const prefs = (over: Partial<ExportPrefs> = {}): ExportPrefs => ({
  include: { ...DEFAULT_EXPORT_PREFS.include },
  onlyUnresolved: false,
  ...over,
});

const annotation = (stale: boolean) => ({ id: "u", stale }) as UiAnnotation;

describe("parseExportPrefs", () => {
  it("defaults every category on, including the checklist", () => {
    expect(parseExportPrefs(null)).toEqual(DEFAULT_EXPORT_PREFS);
  });

  it("keeps defaults for malformed or partial values", () => {
    expect(parseExportPrefs("{not json")).toEqual(DEFAULT_EXPORT_PREFS);
    const p = parseExportPrefs(JSON.stringify({ include: { notes: false, checklist: "no" }, onlyUnresolved: true }));
    expect(p.include.notes).toBe(false);
    expect(p.include.checklist).toBe(true);
    expect(p.onlyUnresolved).toBe(true);
  });

  it("does not share the default object", () => {
    parseExportPrefs(null).include.comments = false;
    expect(DEFAULT_EXPORT_PREFS.include.comments).toBe(true);
  });
});

describe("collectExportItems", () => {
  it("splits threads by kind and flattens the body to one line", () => {
    const items = collectExportItems(ai());
    expect(items.comments.map((i) => i.id)).toEqual(["c1"]);
    expect(items.questions.map((i) => i.id)).toEqual(["q1", "q2"]);
    expect(items.notes[0].label).toBe("body n1 second line");
  });

  it("lists resolved and dropped findings, marked resolved", () => {
    const items = collectExportItems(ai());
    expect(items.findings.map((i) => [i.id, i.resolved])).toEqual([["f1", false], ["f2", true], ["f3", true]]);
  });

  it("tolerates a missing snapshot", () => {
    expect(collectExportItems(null).findings).toEqual([]);
  });
});

describe("buildExportOpts", () => {
  const items = collectExportItems(ai());

  it("sends no allow-list when nothing in a category is excluded", () => {
    const opts = buildExportOpts(prefs(), items, new Set(["q1"]));
    expect(opts.includeQuestionIds).toEqual(["q2"]);
    expect(opts.includeCommentIds).toBeUndefined();
    expect(opts.includeFindingIds).toBeUndefined();
  });

  it("keeps resolved findings in the allow-list so Only unresolved alone decides them", () => {
    const opts = buildExportOpts(prefs(), items, new Set(["f1"]));
    expect(opts.includeFindingIds).toEqual(["f2", "f3"]);
  });

  it("carries the category toggles and the checklist through", () => {
    const p = prefs();
    p.include.checklist = false;
    p.include.notes = false;
    const opts = buildExportOpts(p, items, new Set());
    expect(opts.includeChecklist).toBe(false);
    expect(opts.includeNotes).toBe(false);
  });
});

describe("exportItemCount", () => {
  const items = collectExportItems(ai());

  it("counts picked items and UI annotations, not the checklist", () => {
    // c1, q1, q2, n1, f1, f2, f3 + 2 annotations
    expect(exportItemCount(prefs(), items, new Set(), [annotation(false), annotation(true)])).toBe(9);
  });

  it("drops excluded items and disabled categories", () => {
    const p = prefs();
    p.include.findings = false;
    expect(exportItemCount(p, items, new Set(["q1"]), [])).toBe(3);
  });

  it("leaves out resolved items and stale annotations under Only unresolved", () => {
    expect(exportItemCount(prefs({ onlyUnresolved: true }), items, new Set(), [annotation(false), annotation(true)])).toBe(5);
  });
});

describe("visibleItems", () => {
  it("hides resolved items only under Only unresolved", () => {
    const qs = collectExportItems(ai()).questions;
    expect(visibleItems(qs, false).length).toBe(2);
    expect(visibleItems(qs, true).map((i) => i.id)).toEqual(["q1"]);
  });
});

describe("exportTabKey", () => {
  const tab = (idx: number, branch: string, active: boolean) => ({
    idx, label: branch, kind: "local_branch", branch, pr_number: null, remote: null, repo_root: "/r", is_active: active, change_token: "t",
  });

  it("keys by tab identity, not index", () => {
    const a = exportTabKey({ tabs: [tab(0, "x", false), tab(1, "y", true)], active_tab: 1 } as unknown as AppSnapshot);
    const b = exportTabKey({ tabs: [tab(0, "y", true)], active_tab: 0 } as unknown as AppSnapshot);
    expect(a).toBe(b);
  });

  it("is empty without a snapshot", () => {
    expect(exportTabKey(null)).toBe("");
  });
});

describe("highlightMarkdown", () => {
  it("escapes HTML before highlighting", () => {
    const out = highlightMarkdown("<script>alert(1)</script>");
    expect(out).not.toContain("<script>");
    expect(out).toContain("&lt;script&gt;");
  });

  it("marks headings, quotes, checked items, code and bold", () => {
    const out = highlightMarkdown("## a.ts\n> quoted\n- [x] done\n- [ ] open\nuse `x` and **y**");
    expect(out).toContain('<span class="mh">## a.ts</span>');
    expect(out).toContain('<span class="mq">&gt; quoted</span>');
    expect(out).toContain('<span class="mc">- [x] done</span>');
    expect(out).toContain("- [ ] open");
    expect(out).toContain('<span class="mcode">`x`</span>');
    expect(out).toContain("<b>**y**</b>");
  });
});

describe("previewSize and itemLocation", () => {
  it("reports lines and kilobytes", () => {
    expect(previewSize("a\nb\nc")).toBe("3 lines · 0.0 KB");
    expect(previewSize("x".repeat(2048))).toBe("1 lines · 2.0 KB");
  });

  it("shortens long basenames and shows a dash for no line", () => {
    expect(itemLocation({ id: "i", file: "a/b/averyveryverylongname.ts", line: null, label: "", resolved: false })).toBe("averyveryveryl…:—");
    expect(itemLocation({ id: "i", file: "a/b.ts", line: 4, label: "", resolved: false })).toBe("b.ts:4");
  });
});
