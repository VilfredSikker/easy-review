import { describe, expect, it } from "bun:test";
import {
  basename,
  evidencedGuard,
  filesPreview,
  followUpFor,
  hasReachDetail,
  previewLine,
  priorityDotClass,
  reachPreview,
  summaryPills,
  verdictLabel,
} from "./triageCard";
import type { TriageSnapshot } from "./types";

function makeTriage(overrides: Partial<TriageSnapshot> = {}): TriageSnapshot {
  return {
    fresh: true,
    first_impression: "",
    verdict_primary: "general",
    experts: [],
    rationale: "",
    confidence: "",
    priority_files: [],
    files_changed: 0,
    approx_risk: "",
    domains: [],
    ...overrides,
  };
}

describe("verdictLabel", () => {
  it("names the known verdicts and passes an unknown one through", () => {
    expect(verdictLabel("general")).toBe("General review");
    expect(verdictLabel("skip")).toBe("Skip deep review");
    expect(verdictLabel("something-new")).toBe("something-new");
  });
});

describe("evidencedGuard", () => {
  it("returns the guard only with evidence (ADR 0039)", () => {
    const guard = { kind: "feature_flag", name: "x", evidence: "src/a.ts:1" };
    expect(evidencedGuard(makeTriage({ guard }))).toEqual(guard);
    expect(evidencedGuard(makeTriage({ guard: { ...guard, evidence: "" } }))).toBeNull();
    expect(evidencedGuard(makeTriage({ guard: null }))).toBeNull();
    expect(evidencedGuard(makeTriage())).toBeNull();
  });
});

describe("summaryPills", () => {
  it("orders the pills risk, reach, guard, confidence, size", () => {
    const pills = summaryPills(
      makeTriage({
        approx_risk: "medium",
        reach: "broad",
        guard: { kind: "feature_flag", name: "reportsPage", evidence: "src/+layout.svelte:182" },
        confidence: "medium",
        files_changed: 17,
      }),
    );
    expect(pills.map((p) => p.label)).toEqual([
      "medium risk",
      "broad reach",
      "guarded",
      "medium confidence",
      "17 files",
    ]);
    expect(pills.map((p) => p.tone)).toEqual([
      "risk-med",
      "warning",
      "success",
      "neutral",
      "neutral",
    ]);
    // The guard's evidence is one hover away, since the pill is one word.
    expect(pills[2]?.title).toBe("feature_flag reportsPage · src/+layout.svelte:182");
  });

  it("leaves out what the triage did not record", () => {
    expect(summaryPills(makeTriage())).toEqual([]);
    // Triage from before reach existed carries no reach block.
    expect(summaryPills(makeTriage({ reach: "unknown", files_changed: 2 }))).toEqual([
      { label: "2 files", tone: "neutral" },
    ]);
  });

  it("drops a guard claim without evidence", () => {
    const pills = summaryPills(
      makeTriage({ reach: "isolated", guard: { kind: "feature_flag", name: "x", evidence: "" } }),
    );
    expect(pills.map((p) => p.label)).toEqual(["isolated reach"]);
    expect(pills[0]?.tone).toBe("success");
  });

  it("colours risk by level and reads info or an unknown level as neutral", () => {
    expect(summaryPills(makeTriage({ approx_risk: "high" }))[0]?.tone).toBe("risk-high");
    expect(summaryPills(makeTriage({ approx_risk: "low" }))[0]?.tone).toBe("risk-low");
    const info = summaryPills(makeTriage({ approx_risk: "info" }))[0];
    expect(info).toEqual({
      label: "info risk",
      tone: "neutral",
      title: "Approximate risk of the whole diff",
    });
  });

  it("counts files in the singular for one", () => {
    expect(summaryPills(makeTriage({ files_changed: 1 }))[0]?.label).toBe("1 file");
  });

  it("reads contained reach as the neutral norm", () => {
    expect(summaryPills(makeTriage({ reach: "contained" }))[0]).toMatchObject({
      label: "contained reach",
      tone: "neutral",
    });
  });
});

describe("followUpFor", () => {
  it("runs the full review for a general verdict", () => {
    expect(followUpFor(makeTriage(), "branch")).toEqual({
      kind: "command",
      command: "run_ai_review",
      args: { scope: "branch" },
    });
  });

  it("runs a scoped review with each expert prefixed, and security when none is named", () => {
    expect(
      followUpFor(makeTriage({ verdict_primary: "expert", experts: ["security", "reliability"] }), "s"),
    ).toEqual({
      kind: "command",
      command: "run_ai_scoped_review",
      args: { scope: "s", paths: [], reviewerKinds: ["expert:security", "expert:reliability"] },
    });
    expect(followUpFor(makeTriage({ verdict_primary: "expert" }), "s")).toEqual({
      kind: "command",
      command: "run_ai_scoped_review",
      args: { scope: "s", paths: [], reviewerKinds: ["expert:security"] },
    });
  });

  it("runs the professor review without a focus prompt", () => {
    expect(followUpFor(makeTriage({ verdict_primary: "professor" }), "s")).toEqual({
      kind: "command",
      command: "run_ai_professor_review",
      args: { scope: "s", focusPrompt: null },
    });
  });

  it("opens the launcher for an arena verdict and does nothing for skip or unknown", () => {
    expect(followUpFor(makeTriage({ verdict_primary: "arena" }), "s")).toEqual({ kind: "arena" });
    expect(followUpFor(makeTriage({ verdict_primary: "skip" }), "s")).toBeNull();
    expect(followUpFor(makeTriage({ verdict_primary: "new-kind" }), "s")).toBeNull();
  });
});

describe("hasReachDetail", () => {
  it("is false for unknown reach, and for a known reach with nothing to say", () => {
    expect(hasReachDetail(makeTriage())).toBe(false);
    expect(hasReachDetail(makeTriage({ reach: "contained", reach_reason: "", touch_points: [] }))).toBe(false);
    expect(
      hasReachDetail(
        makeTriage({ reach: "contained", guard: { kind: "feature_flag", name: "x", evidence: "" } }),
      ),
    ).toBe(false);
  });

  it("is true with a reason, a touch point, or an evidenced guard", () => {
    expect(hasReachDetail(makeTriage({ reach: "broad", reach_reason: "Edits the router." }))).toBe(true);
    expect(hasReachDetail(makeTriage({ reach: "broad", touch_points: ["src/router.ts:4"] }))).toBe(true);
    expect(
      hasReachDetail(
        makeTriage({ reach: "isolated", guard: { kind: "feature_flag", name: "x", evidence: "src/a.ts:1" } }),
      ),
    ).toBe(true);
  });
});

describe("reachPreview", () => {
  it("uses the reason, then the first touch point, then nothing", () => {
    expect(reachPreview(makeTriage({ reach_reason: "**Broad**: edits the router.", touch_points: ["t"] }))).toBe(
      "Broad: edits the router.",
    );
    expect(reachPreview(makeTriage({ touch_points: ["src/router.ts:4 — new route"] }))).toBe(
      "src/router.ts:4 — new route",
    );
    expect(reachPreview(makeTriage())).toBe("");
  });
});

describe("previewLine", () => {
  it("takes the first non-empty line with its markup stripped", () => {
    const text = "\n## Gut feel\n\nThe **SQL** and `tests` look [careful](x). Fix `</output>` first.";
    expect(previewLine(text)).toBe("Gut feel");
    expect(previewLine("- The **SQL** and `tests` look [careful](x).")).toBe(
      "The SQL and tests look careful.",
    );
  });

  it("keeps the text of a code span verbatim, stars included", () => {
    expect(previewLine("Matches `src/**/*.ts` only.")).toBe("Matches src/**/*.ts only.");
    expect(previewLine("Adds `get_report_overview` with grants.")).toBe(
      "Adds get_report_overview with grants.",
    );
  });

  it("strips block quotes and numbered lists, and shows an image by its alt text", () => {
    expect(previewLine("> quoted **bold**")).toBe("quoted bold");
    expect(previewLine("1. first `a*b`")).toBe("first a*b");
    expect(previewLine("![diagram](x.png) next")).toBe("diagram next");
  });

  it("skips rules and fences, which carry no words", () => {
    expect(previewLine("---\n\nReal line")).toBe("Real line");
    expect(previewLine("* * *\nAfter the rule")).toBe("After the rule");
    expect(previewLine("```ts\nconst x = 1;\n```")).toBe("const x = 1;");
  });

  it("is empty for an empty text", () => {
    expect(previewLine("")).toBe("");
    expect(previewLine("\n\n")).toBe("");
  });
});

describe("filesPreview", () => {
  it("names the top file and why", () => {
    expect(
      filesPreview([
        { path: "packages/ui/src/routes/+page.svelte", reason: "Stray closing tag.", risk: "high" },
        { path: "other.ts", reason: "", risk: "low" },
      ]),
    ).toBe("+page.svelte · Stray closing tag.");
  });

  it("falls back to the path alone, and to nothing", () => {
    expect(filesPreview([{ path: "src/a.ts", reason: "", risk: "low" }])).toBe("src/a.ts");
    expect(filesPreview([])).toBe("");
  });
});

describe("basename", () => {
  it("drops the directory", () => {
    expect(basename("a/b/c.ts")).toBe("c.ts");
    expect(basename("c.ts")).toBe("c.ts");
  });
});

describe("priorityDotClass", () => {
  it("maps the three levels and mutes the rest", () => {
    expect(priorityDotClass("high")).toBe("bg-risk-high");
    expect(priorityDotClass("medium")).toBe("bg-risk-med");
    expect(priorityDotClass("low")).toBe("bg-risk-low");
    expect(priorityDotClass("info")).toBe("bg-muted");
    expect(priorityDotClass("")).toBe("bg-muted");
  });
});
