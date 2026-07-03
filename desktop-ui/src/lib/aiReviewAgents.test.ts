import { describe, expect, it } from "bun:test";
import type { FlatFinding } from "$lib/types";
import {
  ALL_REVIEWERS,
  agentScopedSummaryLine,
  coerceAgentFilter,
  resolveAgentSummary,
  countBySeverity,
  defaultAgentFilter,
  filterByAgent,
  sortAgentLabels,
  uniqueAgentLabels,
  useAgentScopedSummary,
} from "./aiReviewAgents";

function mkFinding(overrides: Partial<FlatFinding> = {}): FlatFinding {
  return {
    id: "f1",
    file: "a.ts",
    line: 1,
    hunk_index: 0,
    severity: "low",
    expert_label: null,
    agent_label: "General",
    title: "t",
    message_markdown: "",
    promoted_to: null,
    thread_id: null,
    ...overrides,
  };
}

describe("sortAgentLabels", () => {
  it("orders General, experts, then Professor", () => {
    expect(
      sortAgentLabels(["Professor", "Security", "General", "Patterns"]),
    ).toEqual(["General", "Security", "Patterns", "Professor"]);
  });

  it("appends unknown labels alphabetically after known", () => {
    expect(sortAgentLabels(["Zebra", "General"])).toEqual(["General", "Zebra"]);
  });
});

describe("uniqueAgentLabels", () => {
  it("dedupes and sorts", () => {
    const labels = uniqueAgentLabels([
      mkFinding({ agent_label: "Professor" }),
      mkFinding({ id: "f2", agent_label: "Professor" }),
      mkFinding({ id: "f3", agent_label: "General" }),
    ]);
    expect(labels).toEqual(["General", "Professor"]);
  });

  it("includes experts that only contributed a summary, no findings", () => {
    const labels = uniqueAgentLabels(
      [mkFinding({ agent_label: "Patterns" })],
      ["Patterns", "Testing"],
    );
    expect(labels).toEqual(["Testing", "Patterns"]);
  });
});

describe("countBySeverity", () => {
  it("counts high med low", () => {
    expect(
      countBySeverity([
        mkFinding({ severity: "high" }),
        mkFinding({ id: "f2", severity: "med" }),
        mkFinding({ id: "f3", severity: "low" }),
        mkFinding({ id: "f4", severity: "low" }),
      ]),
    ).toEqual({ high: 1, med: 1, low: 2 });
  });
});

describe("filterByAgent", () => {
  it("returns all when filter is all", () => {
    const findings = [
      mkFinding({ agent_label: "General" }),
      mkFinding({ id: "f2", agent_label: "Professor" }),
    ];
    expect(filterByAgent(findings, ALL_REVIEWERS)).toHaveLength(2);
  });

  it("filters to one agent", () => {
    const findings = [
      mkFinding({ agent_label: "General" }),
      mkFinding({ id: "f2", agent_label: "Professor" }),
    ];
    expect(filterByAgent(findings, "Professor")).toHaveLength(1);
  });
});

describe("defaultAgentFilter", () => {
  it("selects sole agent when only one", () => {
    expect(defaultAgentFilter(["Professor"])).toBe("Professor");
  });

  it("selects all when multiple", () => {
    expect(defaultAgentFilter(["General", "Professor"])).toBe(ALL_REVIEWERS);
  });
});

describe("coerceAgentFilter", () => {
  const multi = ["General", "Testing", "Security"];

  it("preserves a specific agent when still present", () => {
    expect(coerceAgentFilter(multi, "Testing")).toBe("Testing");
  });

  it("preserves All reviewers when multiple labels exist", () => {
    expect(coerceAgentFilter(multi, ALL_REVIEWERS)).toBe(ALL_REVIEWERS);
  });

  it("resets to default when current agent is absent", () => {
    expect(coerceAgentFilter(["General", "Security"], "Testing")).toBe(
      ALL_REVIEWERS,
    );
  });

  it("coerces to sole agent when only one label", () => {
    expect(coerceAgentFilter(["Professor"], "Testing")).toBe("Professor");
  });

  it("returns all when labels empty", () => {
    expect(coerceAgentFilter([], "Testing")).toBe(ALL_REVIEWERS);
  });
});

describe("useAgentScopedSummary", () => {
  it("is true for single non-General agent", () => {
    expect(useAgentScopedSummary("Professor")).toBe(true);
    expect(useAgentScopedSummary("General")).toBe(false);
    expect(useAgentScopedSummary(ALL_REVIEWERS)).toBe(false);
  });
});

describe("resolveAgentSummary", () => {
  const counts = { high: 1, med: 0, low: 0 };

  it("uses agent_summaries for specialized agents", () => {
    const r = resolveAgentSummary(
      {
        summary_markdown: "General overview",
        agent_summaries: {
          Testing: "**Coverage gap** on error paths.",
        },
      },
      "Testing",
      counts,
      2,
      false,
    );
    expect(r.markdown).toBe(true);
    expect(r.text).toContain("Coverage gap");
  });

  it("keeps general summary for General filter", () => {
    const r = resolveAgentSummary(
      { summary_markdown: "General overview", agent_summaries: {} },
      "General",
      counts,
      2,
      false,
    );
    expect(r.text).toBe("General overview");
    expect(r.markdown).toBe(true);
  });

  it("falls back to count line when agent summary missing", () => {
    const r = resolveAgentSummary(
      { summary_markdown: null, agent_summaries: {} },
      "Security",
      counts,
      1,
      false,
    );
    expect(r.markdown).toBe(false);
    expect(r.text).toBe("1 finding from Security");
  });
});

describe("agentScopedSummaryLine", () => {
  it("uses insight wording for Professor", () => {
    expect(
      agentScopedSummaryLine("Professor", { high: 0, med: 0, low: 14 }),
    ).toBe("14 insights from Professor");
  });

  it("uses finding wording for experts", () => {
    expect(
      agentScopedSummaryLine("Security", { high: 1, med: 0, low: 0 }),
    ).toBe("1 finding from Security");
  });
});
