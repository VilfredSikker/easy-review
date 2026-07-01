import { describe, expect, it } from "vitest";
import { triageRecommendedPaths } from "./triageSuggestions";
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

describe("triageRecommendedPaths", () => {
  it("returns [] when there is no triage", () => {
    expect(triageRecommendedPaths(null)).toEqual([]);
    expect(triageRecommendedPaths(undefined)).toEqual([]);
  });

  it("returns [] when triage is stale", () => {
    const triage = makeTriage({
      fresh: false,
      priority_files: [{ path: "src/a.rs", reason: "", risk: "high" }],
    });
    expect(triageRecommendedPaths(triage)).toEqual([]);
  });

  it("returns priority file paths in order", () => {
    const triage = makeTriage({
      priority_files: [
        { path: "src/a.rs", reason: "session", risk: "high" },
        { path: "src/b.rs", reason: "parsing", risk: "medium" },
      ],
    });
    expect(triageRecommendedPaths(triage)).toEqual(["src/a.rs", "src/b.rs"]);
  });

  it("de-duplicates repeated paths while preserving first-seen order", () => {
    const triage = makeTriage({
      priority_files: [
        { path: "src/a.rs", reason: "", risk: "high" },
        { path: "src/b.rs", reason: "", risk: "low" },
        { path: "src/a.rs", reason: "again", risk: "high" },
      ],
    });
    expect(triageRecommendedPaths(triage)).toEqual(["src/a.rs", "src/b.rs"]);
  });

  it("skips empty paths", () => {
    const triage = makeTriage({
      priority_files: [
        { path: "", reason: "", risk: "" },
        { path: "src/a.rs", reason: "", risk: "high" },
      ],
    });
    expect(triageRecommendedPaths(triage)).toEqual(["src/a.rs"]);
  });
});
