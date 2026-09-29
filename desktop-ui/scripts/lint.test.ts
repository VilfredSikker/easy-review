import { describe, expect, it } from "bun:test";
import { countByFileRule, ratchet, type Baseline } from "./lint";

const v = (file: string, rule: string) => ({ file, rule });

describe("countByFileRule", () => {
  it("counts per file and rule, sorted", () => {
    expect(
      countByFileRule([v("b.ts", "x"), v("a.ts", "y"), v("a.ts", "x"), v("a.ts", "x")]),
    ).toEqual({
      "a.ts": { x: { count: 2 }, y: { count: 1 } },
      "b.ts": { x: { count: 1 } },
    });
    expect(Object.keys(countByFileRule([v("b.ts", "x"), v("a.ts", "x")]))).toEqual([
      "a.ts",
      "b.ts",
    ]);
  });
});

describe("ratchet", () => {
  const baseline: Baseline = { "a.ts": { x: { count: 2 } } };

  it("passes when counts match the baseline", () => {
    const r = ratchet({ "a.ts": { x: { count: 2 } } }, baseline);
    expect(r.over).toEqual([]);
    expect(r.stale).toEqual([]);
    expect(r.pruned).toEqual(baseline);
  });

  it("fails a pair that grows past its baseline", () => {
    const r = ratchet({ "a.ts": { x: { count: 3 } } }, baseline);
    expect(r.over).toEqual([{ file: "a.ts", rule: "x", count: 3, allowed: 2 }]);
    expect(r.pruned).toEqual(baseline);
  });

  it("fails any violation in a file or rule with no baseline", () => {
    const r = ratchet(
      { "a.ts": { x: { count: 2 }, y: { count: 1 } }, "b.ts": { x: { count: 1 } } },
      baseline,
    );
    expect(r.over.map((o) => `${o.file}:${o.rule}`)).toEqual(["a.ts:y", "b.ts:x"]);
    expect(r.pruned).toEqual(baseline);
  });

  it("does not let one file spend slack from another", () => {
    const r = ratchet({ "b.ts": { x: { count: 1 } } }, baseline);
    expect(r.over).toEqual([{ file: "b.ts", rule: "x", count: 1, allowed: 0 }]);
    expect(r.stale).toEqual([{ file: "a.ts", rule: "x", count: 0, allowed: 2 }]);
  });

  it("reports shrinkage as stale and prunes to the lower count", () => {
    const r = ratchet({ "a.ts": { x: { count: 1 } } }, baseline);
    expect(r.over).toEqual([]);
    expect(r.stale).toEqual([{ file: "a.ts", rule: "x", count: 1, allowed: 2 }]);
    expect(r.pruned).toEqual({ "a.ts": { x: { count: 1 } } });
  });

  it("drops fully fixed entries when pruning", () => {
    expect(ratchet({}, baseline).pruned).toEqual({});
  });

  it("never grows the pruned baseline", () => {
    const r = ratchet({ "a.ts": { x: { count: 5 } }, "c.ts": { z: { count: 1 } } }, baseline);
    expect(r.pruned).toEqual(baseline);
    expect(r.current).toEqual({ "a.ts": { x: { count: 5 } }, "c.ts": { z: { count: 1 } } });
  });
});
