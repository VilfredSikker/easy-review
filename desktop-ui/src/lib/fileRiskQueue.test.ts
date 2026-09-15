import { describe, expect, it } from "bun:test";
import { riskCounts, riskQueueRows, type RiskQueueSort } from "./fileRiskQueue";
import type { FileRiskSnapshot, FileSnapshot } from "./types";

function risk(path: string, level: FileRiskSnapshot["risk"]): FileRiskSnapshot {
  return { path, risk: level, risk_reason: `${level} because`, summary: "" };
}

function file(
  path: string,
  additions: number,
  deletions: number,
  extra: Partial<FileSnapshot> = {},
) {
  // Only the fields the queue reads; the rest of FileSnapshot is irrelevant here.
  return { path, additions, deletions, reviewed: false, ...extra } as FileSnapshot;
}

const risks = [risk("b.rs", "low"), risk("a.rs", "high"), risk("c.rs", "med")];
const files = [
  file("a.rs", 2, 1, { reviewed: true, status: "added", finding_count: 3, comment_count: 0 }),
  file("b.rs", 40, 10, { status: "modified", finding_count: 0, comment_count: 5 }),
  file("c.rs", 5, 5, { status: "deleted", finding_count: 1, question_count: 2 }),
];

describe("riskQueueRows", () => {
  it("orders by risk by default, most severe first", () => {
    expect(riskQueueRows(risks, files, "risk").map((r) => r.path)).toEqual([
      "a.rs",
      "c.rs",
      "b.rs",
    ]);
  });

  it("orders by churn when asked, biggest first", () => {
    expect(riskQueueRows(risks, files, "churn").map((r) => r.path)).toEqual([
      "b.rs",
      "c.rs",
      "a.rs",
    ]);
  });

  it("orders by path when asked", () => {
    expect(riskQueueRows(risks, files, "path").map((r) => r.path)).toEqual([
      "a.rs",
      "b.rs",
      "c.rs",
    ]);
  });

  it("orders structural changes first when asked", () => {
    // added, then deleted, then modified — a file that appeared or went away is
    // where a reader's assumptions are stalest.
    expect(riskQueueRows(risks, files, "status").map((r) => r.path)).toEqual([
      "a.rs",
      "c.rs",
      "b.rs",
    ]);
  });

  it("orders by findings, and by comments plus questions, when asked", () => {
    expect(riskQueueRows(risks, files, "findings").map((r) => r.path)).toEqual([
      "a.rs",
      "c.rs",
      "b.rs",
    ]);
    // c.rs has two questions and no comments; b.rs has five comments.
    expect(riskQueueRows(risks, files, "comments").map((r) => r.path)).toEqual([
      "b.rs",
      "c.rs",
      "a.rs",
    ]);
  });

  it("carries the reviewed mark and the churn the diff knows", () => {
    const rows = riskQueueRows(risks, files, "path");
    expect(rows[0].reviewed).toBe(true);
    expect(rows[0].churn).toBe(3);
    expect(rows[1].reviewed).toBe(false);
  });

  it("keeps a verdict whose file has left the diff", () => {
    const rows = riskQueueRows([...risks, risk("gone.rs", "high")], files, "risk");
    const orphan = rows.find((r) => r.path === "gone.rs");
    expect(orphan).toBeDefined();
    expect(orphan?.churn).toBe(0);
    expect(orphan?.reviewed).toBe(false);
  });

  it("does not mutate the caller's list", () => {
    const input = [...risks];
    riskQueueRows(input, files, "path");
    expect(input.map((r) => r.path)).toEqual(["b.rs", "a.rs", "c.rs"]);
  });
});

describe("riskCounts", () => {
  it("counts per level", () => {
    const sorts: RiskQueueSort[] = ["risk", "churn", "path"];
    expect(riskCounts(riskQueueRows(risks, files, sorts[0]))).toEqual({ high: 1, med: 1, low: 1 });
  });

  it("reports zeroes rather than undefined for an empty queue", () => {
    expect(riskCounts([])).toEqual({ high: 0, med: 0, low: 0 });
  });
});
