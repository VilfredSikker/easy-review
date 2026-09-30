import { describe, expect, it } from "bun:test";
import { orderPrsByStack } from "./prStacks";
import type { PrInfo } from "./types";

function pr(number: number, head_ref: string, base_ref = "main"): PrInfo {
  return {
    number,
    title: `PR ${number}`,
    head_ref,
    state: "OPEN",
    is_draft: false,
    author: "will",
    assignees: [],
    reviewers: [],
    checks_state: null,
    review_decision: null,
    merged_at: null,
    approved_by_me: false,
    base_ref,
    head_oid: "abc",
    updated_at: "",
  };
}

const layout = (prs: PrInfo[]) => orderPrsByStack(prs).map((r) => `${"  ".repeat(r.depth)}#${r.pr.number}`);

describe("orderPrsByStack", () => {
  it("groups a stack bottom-first, indenting each layer under its base", () => {
    // gh lists newest first, so the stack arrives top-first and interleaved.
    const prs = [
      pr(1573, "data-export"),
      pr(1512, "plate-designer", "dev-7332"),
      pr(1511, "dev-7332", "dev-7331"),
      pr(1507, "dev-7331", "dev-7330"),
      pr(1506, "dev-7330"),
    ];
    expect(layout(prs)).toEqual(["#1573", "#1506", "  #1507", "    #1511", "      #1512"]);
  });

  it("leaves unstacked PRs flat and in order", () => {
    expect(layout([pr(3, "c"), pr(1, "a"), pr(2, "b")])).toEqual(["#3", "#1", "#2"]);
  });

  it("puts sibling layers at the same depth under a shared base", () => {
    const prs = [pr(10, "base"), pr(11, "left", "base"), pr(12, "right", "base")];
    expect(layout(prs)).toEqual(["#10", "  #11", "  #12"]);
  });

  it("treats a layer whose base PR is filtered out as a root", () => {
    expect(layout([pr(1507, "dev-7331", "dev-7330")])).toEqual(["#1507"]);
  });

  it("does not nest main-based PRs under a release PR from main", () => {
    const prs = [pr(1, "main", "production"), pr(2, "feat-a"), pr(3, "feat-b")];
    expect(layout(prs)).toEqual(["#1", "#2", "#3"]);
  });

  it("keeps every PR when base/head refs form a cycle", () => {
    const rows = orderPrsByStack([pr(1, "a", "b"), pr(2, "b", "a")]);
    expect(rows.map((r) => r.pr.number).sort()).toEqual([1, 2]);
  });
});
