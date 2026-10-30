import { describe, expect, it } from "bun:test";
import { deriveLiveIndicator } from "./liveIndicator";

describe("deriveLiveIndicator", () => {
  it("is live on a fresh PR diff and names the branch", () => {
    expect(deriveLiveIndicator({ isPrView: true, diffStale: null, branch: "sul/fix" })).toEqual({
      state: "live",
      label: "Live · sul/fix",
    });
  });

  it("is stale when the PR head moved, carrying the reason", () => {
    const r = deriveLiveIndicator({
      isPrView: true,
      diffStale: { message: "origin moved 2 commits" },
      branch: "b",
    });
    expect(r).toEqual({ state: "stale", label: "Stale · origin moved 2 commits" });
  });

  it("is stale without a message", () => {
    expect(deriveLiveIndicator({ isPrView: true, diffStale: {}, branch: "b" }).label).toBe("Stale");
  });

  it("is local on a worktree even if a stale flag lingers", () => {
    expect(
      deriveLiveIndicator({ isPrView: false, diffStale: { message: "x" }, branch: "b" }),
    ).toEqual({ state: "local", label: "Live · local worktree" });
  });

  it("drops the branch suffix when there is none", () => {
    expect(deriveLiveIndicator({ isPrView: true, diffStale: undefined, branch: "" }).label).toBe("Live");
  });
});
