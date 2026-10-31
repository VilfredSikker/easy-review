import { beforeEach, describe, expect, it } from "bun:test";
import type { LineCommits } from "$lib/lineCommits";
import { lineCommitStore } from "./lineCommits.svelte";

function reply(path: string, cacheKey: string): LineCommits {
  return {
    path,
    cache_key: cacheKey,
    unavailable: null,
    commits: ["a".repeat(40)],
    added: [{ start: 1, len: 1, commit: 0 }],
    deleted: [],
    deleted_attributed: true,
  };
}

interface Deferred {
  resolve: (v: LineCommits) => void;
  reject: (e: unknown) => void;
}

/** A fetcher whose replies the test settles by hand, in any order. */
function controlledFetcher() {
  const pending = new Map<string, Deferred>();
  const calls: string[] = [];
  const fetcher = (path: string, cacheKey: string): Promise<LineCommits> =>
    new Promise<LineCommits>((resolve, reject) => {
      calls.push(`${path}@${cacheKey}`);
      pending.set(`${path}@${cacheKey}`, { resolve, reject });
    });
  const settle = (id: string): Deferred => {
    const d = pending.get(id);
    if (!d) throw new Error(`no request for ${id}`);
    return d;
  };
  return { fetcher, settle, calls };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("lineCommitStore", () => {
  beforeEach(() => lineCommitStore.clear());

  it("stores a reply for the key it was asked with", async () => {
    const { fetcher, settle } = controlledFetcher();
    lineCommitStore.request("y.rs", "A", fetcher);
    settle("y.rs@A").resolve(reply("y.rs", "A"));
    await flush();
    expect(lineCommitStore.get("y.rs", "A")?.cache_key).toBe("A");
  });

  it("keeps the newer reply when an older one lands after it", async () => {
    const { fetcher, settle } = controlledFetcher();
    lineCommitStore.request("x.rs", "A", fetcher);
    lineCommitStore.request("x.rs", "B", fetcher);

    settle("x.rs@B").resolve(reply("x.rs", "B"));
    await flush();
    settle("x.rs@A").resolve(reply("x.rs", "A"));
    await flush();

    expect(lineCommitStore.get("x.rs", "B")?.cache_key).toBe("B");
    expect(lineCommitStore.get("x.rs", "A")).toBeNull();
  });

  it("asks again for a superseded key when the file returns to it", async () => {
    const { fetcher, settle, calls } = controlledFetcher();
    lineCommitStore.request("v.rs", "A", fetcher);
    lineCommitStore.request("v.rs", "B", fetcher);
    settle("v.rs@A").resolve(reply("v.rs", "A"));
    await flush();
    lineCommitStore.request("v.rs", "A", fetcher);
    expect(calls).toEqual(["v.rs@A", "v.rs@B", "v.rs@A"]);
  });

  it("keeps the stored reply when the file reverts before a newer reply lands", async () => {
    const { fetcher, settle } = controlledFetcher();
    lineCommitStore.request("u.rs", "A", fetcher);
    settle("u.rs@A").resolve(reply("u.rs", "A"));
    await flush();
    lineCommitStore.request("u.rs", "B", fetcher);
    lineCommitStore.request("u.rs", "A", fetcher);
    settle("u.rs@B").resolve(reply("u.rs", "B"));
    await flush();

    expect(lineCommitStore.get("u.rs", "A")?.cache_key).toBe("A");
    expect(lineCommitStore.get("u.rs", "B")).toBeNull();
  });

  it("stores the in-flight reply when the file reverts to it", async () => {
    const { fetcher, settle } = controlledFetcher();
    lineCommitStore.request("t.rs", "A", fetcher);
    lineCommitStore.request("t.rs", "B", fetcher);
    lineCommitStore.request("t.rs", "A", fetcher);
    settle("t.rs@B").resolve(reply("t.rs", "B"));
    await flush();
    settle("t.rs@A").resolve(reply("t.rs", "A"));
    await flush();

    expect(lineCommitStore.get("t.rs", "A")?.cache_key).toBe("A");
    expect(lineCommitStore.get("t.rs", "B")).toBeNull();
  });

  it("drops a reply that was in flight when the store was cleared", async () => {
    const { fetcher, settle } = controlledFetcher();
    lineCommitStore.request("z.rs", "A", fetcher);
    lineCommitStore.clear();
    settle("z.rs@A").resolve(reply("z.rs", "A"));
    await flush();
    expect(lineCommitStore.get("z.rs", "A")).toBeNull();
  });

  it("does not retry a key that failed", async () => {
    const { fetcher, settle, calls } = controlledFetcher();
    lineCommitStore.request("w.rs", "A", fetcher);
    settle("w.rs@A").reject(new Error("blame failed"));
    await flush();
    lineCommitStore.request("w.rs", "A", fetcher);
    expect(calls).toEqual(["w.rs@A"]);
    expect(lineCommitStore.get("w.rs", "A")).toBeNull();
  });
});
