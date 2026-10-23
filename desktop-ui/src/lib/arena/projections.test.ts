import { describe, expect, it } from "bun:test";
import { finalTruthFindings, pendingAcceptanceFindings } from "./projections";

const findings = [
  { id: "kept", verdict: "kept" as const },
  { id: "escalated", verdict: "escalated" as const },
  { id: "merged", verdict: { merged: { into: "kept" } } },
  { id: "dropped", verdict: "dropped" as const },
  { id: "pending", verdict: "pending" as const },
  { id: "accepted", verdict: "kept" as const, accepted_at: "now" },
];

describe("pendingAcceptanceFindings", () => {
  it("limits flow acceptance to unaccepted kept and escalated survivors", () => {
    expect(finalTruthFindings(findings, true).map((finding) => finding.id)).toEqual([
      "kept",
      "escalated",
      "accepted",
    ]);
    expect(pendingAcceptanceFindings(findings, ["escalated"], true).map((finding) => finding.id)).toEqual(["kept"]);
  });

  it("preserves manual arena acceptance behavior for merged findings", () => {
    expect(pendingAcceptanceFindings(findings).map((finding) => finding.id)).toEqual([
      "kept",
      "escalated",
      "merged",
    ]);
  });

  it("shows every seeded verification verdict without changing flow or manual projections", () => {
    expect(finalTruthFindings(findings, false, true).map((finding) => finding.id)).toEqual([
      "kept",
      "escalated",
      "merged",
      "dropped",
      "pending",
      "accepted",
    ]);
    expect(finalTruthFindings(findings, true).map((finding) => finding.id)).toEqual([
      "kept",
      "escalated",
      "accepted",
    ]);
    expect(finalTruthFindings(findings).map((finding) => finding.id)).toEqual([
      "kept",
      "escalated",
      "merged",
      "accepted",
    ]);
  });
});
