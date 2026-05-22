import { describe, expect, it } from "bun:test";
import { splitRows } from "$lib/splitRows";
import type { LineSnapshot } from "$lib/types";

const ctx = (oldN: number, newN: number, text = "ctx"): LineSnapshot => ({
  old_num: oldN,
  new_num: newN,
  kind: "context",
  text,
  spans: [{ text, color: "" }],
});
const add = (newN: number, text = "add"): LineSnapshot => ({
  old_num: null,
  new_num: newN,
  kind: "add",
  text,
  spans: [{ text, color: "" }],
});
const del = (oldN: number, text = "del"): LineSnapshot => ({
  old_num: oldN,
  new_num: null,
  kind: "del",
  text,
  spans: [{ text, color: "" }],
});

describe("splitRows", () => {
  it("pairs equal-length del/add runs (modify pair)", () => {
    // 2 context + 2 del + 2 add → 4 rows (2 ctx + 2 paired)
    const rows = splitRows([
      ctx(1, 1),
      ctx(2, 2),
      del(3),
      del(4),
      add(3),
      add(4),
    ]);
    expect(rows.length).toBe(4);
    expect(rows[0].left?.kind).toBe("context");
    expect(rows[0].right?.kind).toBe("context");
    expect(rows[1].left?.kind).toBe("context");
    expect(rows[2].left?.kind).toBe("del");
    expect(rows[2].right?.kind).toBe("add");
    expect(rows[3].left?.kind).toBe("del");
    expect(rows[3].right?.kind).toBe("add");
  });

  it("handles a mixed run with extra adds", () => {
    // 1 del + 2 add → 2 rows: row0 = (del, add); row1 = (null, add)
    const rows = splitRows([del(1), add(1), add(2)]);
    expect(rows.length).toBe(2);
    expect(rows[0].left?.kind).toBe("del");
    expect(rows[0].right?.kind).toBe("add");
    expect(rows[1].left).toBeNull();
    expect(rows[1].right?.kind).toBe("add");
  });

  it("passes context-only hunks through unchanged", () => {
    const lines = [ctx(1, 1, "a"), ctx(2, 2, "b"), ctx(3, 3, "c")];
    const rows = splitRows(lines);
    expect(rows.length).toBe(3);
    for (let i = 0; i < 3; i++) {
      expect(rows[i].left).toBe(lines[i]);
      expect(rows[i].right).toBe(lines[i]);
    }
  });

  it("places a standalone add on the right with empty left", () => {
    const rows = splitRows([ctx(1, 1), add(2)]);
    expect(rows.length).toBe(2);
    expect(rows[1].left).toBeNull();
    expect(rows[1].right?.kind).toBe("add");
  });

  it("places a standalone del on the left with empty right", () => {
    const rows = splitRows([ctx(1, 1), del(2), ctx(3, 2)]);
    expect(rows.length).toBe(3);
    expect(rows[1].left?.kind).toBe("del");
    expect(rows[1].right).toBeNull();
  });
});
