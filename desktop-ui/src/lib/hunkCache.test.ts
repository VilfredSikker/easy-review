import { describe, expect, it } from "bun:test";
import type { Hunk } from "$lib/protocol/generated/Hunk";
import { HunkCache, hunkBytes } from "./hunkCache";

function hunks(lines: number, text = "x".repeat(40)): Hunk[] {
  const body = Array.from({ length: lines }, (_, i) => ({ old_num: i + 1, new_num: i + 1, kind: "context" as const, text }));
  return [{ header: "@@ -1 +1 @@", old_start: 1, old_count: lines, new_start: 1, new_count: lines, lines: body }];
}

describe("HunkCache", () => {
  it("stays under its byte bound across 500 files", () => {
    const one = hunkBytes(hunks(50));
    const cache = new HunkCache(one * 40);
    for (let i = 0; i < 500; i++) {
      cache.put(`h${i}`, hunks(50));
      expect(cache.bytes).toBeLessThanOrEqual(one * 40);
    }
    expect(cache.size).toBe(40);
    expect(cache.has("h499")).toBe(true);
    expect(cache.has("h0")).toBe(false);
  });

  it("drops the least recently read first", () => {
    const one = hunkBytes(hunks(10));
    const cache = new HunkCache(one * 2);
    cache.put("a", hunks(10));
    cache.put("b", hunks(10));
    expect(cache.get("a")).toBeDefined();
    cache.put("c", hunks(10));
    expect([cache.has("a"), cache.has("b"), cache.has("c")]).toEqual([true, false, true]);
  });

  it("keeps one entry larger than the bound rather than nothing", () => {
    const cache = new HunkCache(10);
    cache.put("big", hunks(10));
    expect(cache.has("big")).toBe(true);
    cache.put("next", hunks(10));
    expect([cache.has("big"), cache.has("next"), cache.size]).toEqual([false, true, 1]);
  });
});
