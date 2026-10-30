import { describe, expect, it } from "vitest";
import type { FlatFinding, ThreadSnapshot } from "$lib/types";
import { DEFAULT_COMMENT_VISIBILITY } from "$lib/commentVisibility";
import {
  arbiterSummary,
  clip,
  findingLens,
  findingLocation,
  findingsPanelView,
  scopeToReviewer,
  showReviewerPicker,
  toggleSeverity,
} from "./findingsPanel";
import {
  commentRows,
  kindCounts,
  lineRange,
  loadKindFilter,
  panelThreads,
  replyTally,
  threadAudience,
  threadExcerpt,
  COMMENTS_KIND_KEY,
} from "./commentsPanel";
import { groupRiskRows, riskRows, tierCounts, tierHeading, unreviewedHigh } from "./risksPanel";
import { migrateLegacyStoredTab, migrateRightPanelTab, isSecondaryTab } from "./rightPanelTabs";

function finding(over: Partial<FlatFinding>): FlatFinding {
  return {
    id: "f",
    file: "src/a.ts",
    line: 1,
    hunk_index: 0,
    severity: "med",
    confidence: "confirmed",
    lens_category: "general · correctness",
    resolved: false,
    expert_label: null,
    agent_label: "General",
    raised_by: ["general"],
    providers: [],
    security_stage: false,
    title: "t",
    message_markdown: "",
    promoted_to: null,
    thread_id: null,
    ...over,
  };
}

function thread(over: Partial<ThreadSnapshot>): ThreadSnapshot {
  return {
    id: "t",
    kind: "comment",
    file: "src/a.ts",
    line: 1,
    source: "local",
    synced: false,
    stale: false,
    resolved: false,
    root: { id: "r", author: "ann", kind: "human", timestamp: "", body_markdown: "hi" },
    replies: [],
    promoted_to: null,
    ...over,
  };
}

class MemoryStorage {
  private map = new Map<string, string>();
  getItem(k: string) {
    return this.map.get(k) ?? null;
  }
  setItem(k: string, v: string) {
    this.map.set(k, v);
  }
  removeItem(k: string) {
    this.map.delete(k);
  }
}

describe("right panel tabs", () => {
  it("maps the retired tab names onto the tabs that hold their content", () => {
    expect(migrateRightPanelTab("review")).toBe("findings");
    expect(migrateRightPanelTab("notes")).toBe("comments");
    expect(migrateRightPanelTab("context")).toBe("context");
    expect(migrateRightPanelTab("branch")).toBe("branch");
    expect(migrateRightPanelTab("bogus")).toBe("findings");
    expect(migrateRightPanelTab(null)).toBe("findings");
  });

  it("opens a stored four-tab default on Findings", () => {
    expect(migrateLegacyStoredTab("branch")).toBe("findings");
    expect(migrateLegacyStoredTab("notes")).toBe("comments");
  });

  it("knows which tabs live behind More", () => {
    expect(isSecondaryTab("context")).toBe(true);
    expect(isSecondaryTab("risks")).toBe(false);
  });
});

describe("findingsPanelView", () => {
  const open = [
    finding({ id: "a", severity: "low", file: "z.ts" }),
    finding({ id: "b", severity: "high", confidence: "tentative" }),
    finding({ id: "c", severity: "high", confidence: "informational" }),
    finding({ id: "d", severity: "med", file: "b.ts" }),
  ];
  const resolved = [finding({ id: "r", severity: "high", resolved: true })];

  it("orders by severity then trust, counting open findings before any filter", () => {
    const v = findingsPanelView(open, resolved, { severity: "all", minTrust: "informational", showResolved: false });
    expect(v.rows.map((f) => f.id)).toEqual(["b", "c", "d", "a"]);
    expect(v.counts).toEqual({ high: 2, med: 1, low: 1 });
    expect(v.openCount).toBe(4);
    expect(v.fileCount).toBe(3);
    expect(v.resolvedCount).toBe(1);
    expect(v.gatedCount).toBe(0);
  });

  it("counts what the gate holds back within the tile filter", () => {
    const v = findingsPanelView(open, resolved, { severity: "high", minTrust: "tentative", showResolved: false });
    expect(v.rows.map((f) => f.id)).toEqual(["b"]);
    expect(v.gatedCount).toBe(1);
  });

  it("sinks resolved rows below every open one when shown", () => {
    const v = findingsPanelView(open, resolved, { severity: "high", minTrust: "informational", showResolved: true });
    expect(v.rows.map((f) => f.id)).toEqual(["b", "c", "r"]);
  });

  it("toggles a tile off when clicked again", () => {
    expect(toggleSeverity("all", "high")).toBe("high");
    expect(toggleSeverity("high", "high")).toBe("all");
    expect(toggleSeverity("high", "low")).toBe("low");
  });

  it("formats the row's location and lens", () => {
    expect(findingLocation("short.ts", 12)).toBe("short.ts:12");
    expect(findingLocation("a-very-long-file-name-indeed.ts", null)).toBe("a-very-long-file-name-…");
    expect(clip("abc", 5)).toBe("abc");
    expect(findingLens(finding({ lens_category: "security · injection" }))).toBe("injection");
    expect(findingLens(finding({ lens_category: "", raised_by: ["general", "security"] }))).toBe("security");
  });

  it("summarises arbiter rulings, or says nothing", () => {
    expect(arbiterSummary({ arbiter_dropped: 2, arbiter_merged: 0, arbiter_regraded: 1 })).toBe(
      "2 dropped, 1 regraded by arbiter",
    );
    expect(arbiterSummary({ arbiter_dropped: 0, arbiter_merged: 0, arbiter_regraded: 0 })).toBeNull();
  });
});

describe("reviewer scope", () => {
  const general = finding({ id: "g", agent_label: "General", raised_by: ["general"] });
  const security = finding({ id: "s", agent_label: "Security", expert_label: "Security", raised_by: ["expert:security"] });
  const resolvedSec = finding({ id: "rs", agent_label: "Security", raised_by: ["expert:security"], resolved: true });
  const ai = { findings: [general, security], resolved_findings: [resolvedSec], agent_summaries: { Testing: "ok" } };

  it("filters open and resolved rows the way the diff does", () => {
    const scope = scopeToReviewer(ai, "Security");
    expect(scope.open.map((f) => f.id)).toEqual(["s"]);
    expect(scope.resolved.map((f) => f.id)).toEqual(["rs"]);
    expect(scopeToReviewer(ai, "all").open).toHaveLength(2);
  });

  it("offers every reviewer, including summary-only ones", () => {
    expect(scopeToReviewer(ai, "all").labels).toEqual(["General", "Security", "Testing"]);
    expect(scopeToReviewer(null, "all")).toEqual({ open: [], resolved: [], labels: [] });
  });

  it("shows the picker for a choice or a filter that hides rows", () => {
    expect(showReviewerPicker(["General", "Security"], "all")).toBe(true);
    expect(showReviewerPicker(["General"], "General")).toBe(false);
    expect(showReviewerPicker(["General"], "all")).toBe(false);
    expect(showReviewerPicker(["General"], "Security")).toBe(true);
  });
});

describe("comments panel", () => {
  const threads = [
    thread({ id: "1", kind: "note", file: "b.ts", line: 4 }),
    thread({ id: "2", kind: "comment", file: "a.ts", line: 9, resolved: true }),
    thread({ id: "3", kind: "question", file: "a.ts", line: 2 }),
    thread({ id: "4", kind: "comment", file: "zz.ts", line: 1 }),
  ];

  it("sorts unresolved first, then diff file order, then line", () => {
    expect(commentRows(threads, "all", ["a.ts", "b.ts"]).map((t) => t.id)).toEqual(["3", "1", "4", "2"]);
    expect(commentRows(threads, "comment", ["a.ts", "b.ts"]).map((t) => t.id)).toEqual(["4", "2"]);
  });

  it("counts each kind", () => {
    expect(kindCounts(threads)).toEqual({ all: 4, comment: 2, question: 1, note: 1 });
  });

  it("formats the compact row's parts", () => {
    expect(lineRange(12)).toBe("12");
    expect(lineRange(12, 12)).toBe("12");
    expect(lineRange(12, 18)).toBe("12–18");
    expect(threadAudience("comment")).toBe("GitHub");
    expect(threadAudience("note")).toBe("only you");
    expect(threadExcerpt("a\n\nb   c")).toBe("a b c");
    expect(threadExcerpt("x".repeat(120)).length).toBe(109);
    const replies = [
      { id: "a", author: "x", kind: "ai" as const, timestamp: "", body_markdown: "" },
      { id: "b", author: "y", kind: "you" as const, timestamp: "", body_markdown: "" },
    ];
    expect(replyTally({ replies })).toEqual({ replies: 2, ai: 1 });
  });

  it("hides GitHub comments the diff hides, and keeps every private thread", () => {
    const vis = { ...DEFAULT_COMMENT_VISIBILITY };
    const listed = panelThreads(
      [...threads, thread({ id: "5", kind: "question", resolved: true })],
      vis,
    ).map((t) => t.id);
    expect(listed).toEqual(["4", "1", "3", "5"]);
  });

  it("carries the old Notes sub-filter over once", () => {
    const s = new MemoryStorage();
    s.setItem("notesPanelSubTab", "questions");
    expect(loadKindFilter(s)).toBe("question");
    expect(s.getItem("notesPanelSubTab")).toBeNull();
    expect(s.getItem(COMMENTS_KIND_KEY)).toBe("question");
    s.setItem(COMMENTS_KIND_KEY, "note");
    expect(loadKindFilter(s)).toBe("note");
  });
});

describe("risksPanel", () => {
  const files = [
    { path: "db/1.sql", reviewed: false },
    { path: "src/app.ts", reviewed: true },
    { path: "src/util.ts", reviewed: false, risk: "low" as const },
    { path: "README.md", reviewed: false },
  ];
  const resolved = [
    { path: "db/1.sql", tier: "high" as const, rule: { key: "db/**", label: "Migrations", source: "project" as const } },
    { path: "src/app.ts", tier: "ai" as const, rule: { key: "*.ts", label: null, source: "user" as const } },
    { path: "src/util.ts", tier: "ai" as const, rule: null },
  ];
  const ai = [{ path: "src/app.ts", risk: "high" as const, risk_reason: "Auth path", summary: "" }];

  it("lets a declared tier win and the review rate only ai files", () => {
    const rows = riskRows(files, resolved, ai);
    expect(rows.map((r) => [r.path, r.shown])).toEqual([
      ["db/1.sql", "high"],
      ["src/app.ts", "high"],
      ["src/util.ts", "low"],
      ["README.md", "ai"],
    ]);
    expect(rows[0].aiReason).toBeNull();
    expect(rows[1].aiReason).toBe("Auth path");
    expect(rows[1].rule?.key).toBe("*.ts");
    expect(rows[3].rule).toBeNull();
  });

  it("does not let the review override a declared tier", () => {
    const rows = riskRows(
      [{ path: "db/1.sql", reviewed: false }],
      resolved,
      [{ path: "db/1.sql", risk: "low", risk_reason: "small", summary: "" }],
    );
    expect(rows[0].shown).toBe("high");
    expect(rows[0].aiReason).toBeNull();
  });

  it("groups by tier and counts the unreviewed high files", () => {
    const rows = riskRows(files, resolved, ai);
    expect(groupRiskRows(rows).map((g) => [g.tier, g.rows.length])).toEqual([
      ["high", 2],
      ["low", 1],
      ["ai", 1],
    ]);
    expect(tierCounts(rows)).toEqual({ high: 2, med: 0, low: 1, ai: 1 });
    expect(unreviewedHigh(rows)).toBe(1);
    expect(tierHeading("ai")).toBe("AI · not rated yet");
  });
});
