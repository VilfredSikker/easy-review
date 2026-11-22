import { describe, expect, it } from "bun:test";
import type { Hunk } from "$lib/protocol/generated/Hunk";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import { HunkCache } from "./hunkCache";
import { HunkLoader } from "./hunkLoader";

const HUNKS: Hunk[] = [{ header: "@@ -1 +1 @@", old_start: 1, old_count: 1, new_start: 1, new_count: 1, lines: [] }];

function loader(answer: (keys: ResourceKey[]) => unknown) {
  const cache = new HunkCache();
  const fetched: ResourceKey[][] = [];
  const refetched: number[] = [];
  let changed = 0;
  const load = new HunkLoader({
    cache,
    fetch: async (keys) => {
      fetched.push(keys);
      return answer(keys) as never;
    },
    refetchDiff: (tab) => refetched.push(tab),
    changed: () => (changed += 1),
  });
  return { cache, fetched, refetched, load, changes: () => changed };
}

const hunksOf = (keys: ResourceKey[], hash?: (asked: string) => string) =>
  keys.map((key) => {
    const asked = key.kind === "hunks" ? key.hash : "";
    return { key, version: 0, value: { kind: "hunks", hash: hash ? hash(asked) : asked, hunks: HUNKS } };
  });

describe("HunkLoader", () => {
  it("asks for every missing hash of a window in one fetch, and nothing it holds", async () => {
    const { cache, fetched, load, changes } = loader((keys) => hunksOf(keys));
    cache.put("held", HUNKS);
    await load.load(3, [{ path: "a", hash: "h1" }, { path: "b", hash: "held" }, { path: "c", hash: "h2" }]);
    expect(fetched).toEqual([[{ kind: "hunks", tab: 3, path: "a", hash: "h1" }, { kind: "hunks", tab: 3, path: "c", hash: "h2" }]]);
    expect([cache.has("h1"), cache.has("h2"), changes()]).toEqual([true, true, 1]);
    await load.load(3, [{ path: "a", hash: "h1" }]);
    expect(fetched.length).toBe(1);
  });

  it("keeps a parsed stub under the hash the next diff names too", async () => {
    const { cache, load } = loader((keys) => hunksOf(keys, () => "parsed"));
    await load.load(3, [{ path: "a", hash: "stub" }]);
    expect([cache.has("stub"), cache.has("parsed")]).toEqual([true, true]);
  });

  it("fetches the diff again when a hash names nothing, and caches nothing", async () => {
    const { cache, refetched, load } = loader(() => {
      throw Object.assign(new Error("a.rs changed since the diff was read"), { kind: "stale" });
    });
    await load.load(5, [{ path: "a", hash: "old" }]);
    expect([refetched, cache.size]).toEqual([[5], 0]);
  });
});
