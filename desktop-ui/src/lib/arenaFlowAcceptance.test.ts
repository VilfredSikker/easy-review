import { describe, expect, it } from "bun:test";
import { acceptFlowFindings, arenaAcceptanceRoute } from "./arenaFlowAcceptance";

describe("arena acceptance origin", () => {
  it("keeps flow acceptance bound to its captured bucket after a tab switch", () => {
    expect(arenaAcceptanceRoute({ erDir: "/repo-a/.er", runId: "flow-run" }, "flow-run", ["f1"])).toEqual({
      kind: "flow",
      erDir: "/repo-a/.er",
      runId: "flow-run",
      findingIds: ["f1"],
    });
  });

  it("keeps manual acceptance on the arena command only when no flow origin exists", () => {
    expect(arenaAcceptanceRoute(null, "manual-run", ["f2"])).toEqual({
      kind: "manual",
      request: { run_id: "manual-run", finding_ids: ["f2"] },
    });
    expect(() => arenaAcceptanceRoute({ erDir: "/repo-a/.er", runId: "flow-run" }, "other-run")).toThrow("does not match");
  });

  it("propagates a rejected command and does not read a success receipt", async () => {
    const readReceipt = async () => {
      throw new Error("receipt must not be read after command failure");
    };
    await expect(acceptFlowFindings(
      { erDir: "/repo-a/.er", runId: "flow-run" },
      "flow-run",
      ["f1"],
      async () => { throw new Error("backend rejected acceptance"); },
      readReceipt,
    )).rejects.toThrow("backend rejected acceptance");
  });

  it("counts unique selected IDs recorded as accepted by the pinned run", async () => {
    const count = await acceptFlowFindings(
      { erDir: "/repo-a/.er", runId: "flow-run" },
      "flow-run",
      ["f1", "f1", "f2"],
      async () => {},
      async () => ({ run: { id: "flow-run", findings: [{ id: "f1", accepted_at: "now" }, { id: "f2" }] } }),
    );
    expect(count).toBe(1);
  });
});
