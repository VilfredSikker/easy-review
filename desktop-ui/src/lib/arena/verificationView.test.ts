import { describe, expect, it } from "bun:test";
import type { ArenaFinding, Verdict } from "$lib/types/arena";
import type { HunkSnapshot } from "$lib/types";
import { ARBITER_ROUND } from "./arenaView";
import { MOCK_ARENA_SNAPSHOT } from "./mockRun";
import {
  evidenceExcerpt,
  lensNames,
  matchesFilter,
  regrade,
  splitReasoning,
  stepSelection,
  verificationGroups,
  verificationMeta,
  verificationNote,
  visibleOrder,
} from "./verificationView";

function f(id: string, verdict: Verdict, extra: Partial<ArenaFinding> = {}): ArenaFinding {
  return {
    id,
    file: "src/a.ts",
    line: 10,
    title: id,
    body: "",
    severity_by_round: { 1: "low" },
    raised_by: [],
    verdict,
    confidence: 0.5,
    rationale: "",
    rounds: [],
    ...extra,
  };
}

const MIXED = [
  f("k1", "kept"),
  f("d1", "dropped"),
  f("e1", "escalated"),
  f("m1", { merged: { into: "k1" } }),
  f("k2", "kept"),
];

describe("verificationGroups", () => {
  it("orders escalated, kept, merged, dropped and skips empty groups", () => {
    const groups = verificationGroups(MIXED, "all");
    expect(groups.map((g) => g.key)).toEqual(["escalated", "kept", "merged", "dropped"]);
    expect(groups[1]!.findings.map((x) => x.id)).toEqual(["k1", "k2"]);
  });

  it("keeps only the filtered verdict", () => {
    expect(verificationGroups(MIXED, "merged").map((g) => g.key)).toEqual(["merged"]);
    expect(matchesFilter(MIXED[3]!, "merged")).toBe(true);
    expect(matchesFilter(MIXED[3]!, "kept")).toBe(false);
  });

  it("walks the list in display order", () => {
    expect(visibleOrder(MIXED, "all")).toEqual(["e1", "k1", "k2", "m1", "d1"]);
  });
});

describe("stepSelection", () => {
  const order = ["a", "b", "c"];
  it("moves and clamps at both ends", () => {
    expect(stepSelection(order, "b", 1)).toBe("c");
    expect(stepSelection(order, "c", 1)).toBe("c");
    expect(stepSelection(order, "a", -1)).toBe("a");
  });
  it("starts at the top when nothing visible is selected", () => {
    expect(stepSelection(order, null, 1)).toBe("a");
    expect(stepSelection(order, "gone", -1)).toBe("a");
    expect(stepSelection([], "a", 1)).toBeNull();
  });
});

describe("regrade", () => {
  it("reports a change between first and last round", () => {
    expect(regrade({ severity_by_round: { 2: "med", 1: "low" } })).toBe("low → med");
  });
  it("is null when the severity held or only one round ran", () => {
    expect(regrade({ severity_by_round: { 1: "high", 2: "high" } })).toBeNull();
    expect(regrade({ severity_by_round: { 1: "high" } })).toBeNull();
  });
});

describe("lensNames", () => {
  const reviewers = [
    { id: "g", name: "General" },
    { id: "s", name: "Security" },
    { id: "t", name: "Tests" },
  ];
  it("names one or two lenses and counts the rest", () => {
    expect(lensNames({ raised_by: ["g"] }, reviewers)).toBe("General");
    expect(lensNames({ raised_by: ["g", "s"] }, reviewers)).toBe("General + Security");
    expect(lensNames({ raised_by: ["g", "s", "t"] }, reviewers)).toBe("General +2");
    expect(lensNames({ raised_by: [] }, reviewers)).toBe("Finder");
  });
});

describe("splitReasoning", () => {
  it("shows a short rationale whole with nothing to expand", () => {
    expect(splitReasoning({ rationale: "Holds.", rounds: [] })).toEqual({ why: "Holds.", full: "" });
  });

  it("continues a long rationale instead of repeating its opening", () => {
    const first = "The finder's concern holds and the base ref is read with ls-tree. ".repeat(2).trim();
    const rest = "Regraded because it gives false confidence about the exact failure this fixes.";
    const text = `${first} ${"Nothing queries staging. ".repeat(6)}${rest}`;
    const { why, full } = splitReasoning({ rationale: text, rounds: [] });
    expect(why.length).toBeLessThanOrEqual(220);
    expect(text.startsWith(why)).toBe(true);
    expect(full.endsWith(rest)).toBe(true);
    expect(`${why} ${full}`.replace(/\s+/g, " ")).toBe(text.replace(/\s+/g, " "));
  });

  it("falls back to the arbiter's ballot note and appends a distinct one", () => {
    const rounds = [{ n: ARBITER_ROUND, log: [{ reviewer: "arbiter", vote: "keep" as const, note: "Reproduced." }] }];
    expect(splitReasoning({ rationale: "", rounds })).toEqual({ why: "Reproduced.", full: "" });
    expect(splitReasoning({ rationale: "Kept.", rounds })).toEqual({ why: "Kept.", full: "Reproduced." });
    expect(splitReasoning({ rationale: "Kept. Reproduced.", rounds })).toEqual({ why: "Kept. Reproduced.", full: "" });
  });
});

describe("evidenceExcerpt", () => {
  const hunks: Pick<HunkSnapshot, "lines">[] = [
    {
      lines: [
        { old_num: 8, new_num: 8, kind: "context", text: "a" },
        { old_num: 9, new_num: null, kind: "del", text: "old" },
        { old_num: null, new_num: 9, kind: "add", text: "b" },
        { old_num: 10, new_num: 10, kind: "context", text: "c" },
        { old_num: 11, new_num: 11, kind: "context", text: "d" },
        { old_num: 12, new_num: 12, kind: "context", text: "e" },
      ],
    },
  ];

  it("reads the line around the finding from the new side", () => {
    expect(evidenceExcerpt(hunks, { file: "src/a.ts", line: 10 })).toEqual([
      { num: 9, text: "b" },
      { num: 10, text: "c" },
      { num: 11, text: "d" },
    ]);
  });

  it("prefers evidence ranges in the same file", () => {
    const ev = [
      { file: "src/a.ts", line_start: 11, line_end: 12, note: "" },
      { file: "other.ts", line_start: 8, note: "" },
    ];
    expect(evidenceExcerpt(hunks, { file: "src/a.ts", line: 10, evidence: ev }).map((l) => l.num)).toEqual([11, 12]);
  });

  it("is empty when the line is not in a loaded hunk or there is no line", () => {
    expect(evidenceExcerpt(hunks, { file: "src/a.ts", line: 400 })).toEqual([]);
    expect(evidenceExcerpt(hunks, { file: "src/a.ts", line: null })).toEqual([]);
  });
});

describe("verificationNote", () => {
  const base = { seeded: false, status: "complete" as const, readOnlyFlow: false, awaiting: 0, seededText: "Applied." };
  it("tells a seeded run's outcome by status", () => {
    expect(verificationNote({ ...base, seeded: true })).toEqual({ tone: "ok", text: "✓ Applied." });
    expect(verificationNote({ ...base, seeded: true, status: "failed" }).tone).toBe("err");
  });
  it("asks for an accept only while flow results wait", () => {
    expect(verificationNote({ ...base, awaiting: 3 }).tone).toBe("warn");
    expect(verificationNote(base).tone).toBe("ok");
    expect(verificationNote({ ...base, readOnlyFlow: true, awaiting: 3 }).tone).toBe("info");
    expect(verificationNote({ ...base, status: { running: { round: 1 } } }).tone).toBe("info");
  });
});

describe("verificationMeta", () => {
  it("lists run, findings, lenses, verifier, rounds and the diff it was built from", () => {
    const meta = verificationMeta(MOCK_ARENA_SNAPSHOT);
    const run = MOCK_ARENA_SNAPSHOT.run;
    expect(meta).toContain(`Review run ${run.id}`);
    expect(meta).toContain(`${run.findings.length} findings from ${run.reviewers.length} lenses`);
    expect(meta).toContain(`verifier ${run.config.arbiter.model_id}`);
    expect(meta).toContain("rounds");
    expect(meta).toContain("built from mock-di");
  });
});
