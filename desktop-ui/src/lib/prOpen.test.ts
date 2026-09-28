import { describe, expect, it } from "bun:test";
import { buildPrHint, createPrPrefetch, remoteParts, shouldReplaceTab } from "./prOpen";
import type { PrInfo } from "./types";

function pr(overrides: Partial<PrInfo> = {}): PrInfo {
  return {
    number: 7,
    title: "Fix it",
    author: "octo",
    updated_at: "2026-09-01T00:00:00Z",
    base_ref: "main",
    head_ref: "fix",
    head_oid: "abc123",
    ...overrides,
  } as PrInfo;
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("shouldReplaceTab", () => {
  it("replaces on a plain click and opens a new tab on modifier or middle click", () => {
    const click = (o: Partial<MouseEvent>) => ({ metaKey: false, ctrlKey: false, button: 0, ...o }) as MouseEvent;
    expect(shouldReplaceTab(click({}))).toBe(true);
    expect(shouldReplaceTab(click({ metaKey: true }))).toBe(false);
    expect(shouldReplaceTab(click({ ctrlKey: true }))).toBe(false);
    expect(shouldReplaceTab(click({ button: 1 }))).toBe(false);
  });
});

describe("remoteParts", () => {
  it("parses owner/repo from a GitHub URL or bare slug", () => {
    expect(remoteParts({ remote: "https://github.com/a/b.git" })).toEqual({ owner: "a", repo: "b" });
    expect(remoteParts({ remote: " a/b/ " })).toEqual({ owner: "a", repo: "b" });
  });

  it("returns null when the remote is missing or incomplete", () => {
    expect(remoteParts({ remote: null })).toBeNull();
    expect(remoteParts({ remote: "  " })).toBeNull();
    expect(remoteParts({ remote: "https://github.com/a" })).toBeNull();
  });
});

describe("buildPrHint", () => {
  it("maps the PR fields to the camelCase hint", () => {
    expect(buildPrHint(pr())).toEqual({
      baseRef: "main",
      headRef: "fix",
      headOid: "abc123",
      updatedAt: "2026-09-01T00:00:00Z",
      title: "Fix it",
      author: "octo",
    });
  });

  it("drops the hint when a ref or oid is blank", () => {
    expect(buildPrHint(pr({ head_oid: " " }))).toBeUndefined();
    expect(buildPrHint(pr({ base_ref: "" }))).toBeUndefined();
    expect(buildPrHint(pr({ head_ref: undefined as unknown as string }))).toBeUndefined();
  });
});

describe("createPrPrefetch", () => {
  function recorder() {
    const calls: [string, Record<string, unknown>][] = [];
    const invokeFn = (cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      return Promise.resolve();
    };
    return { calls, prefetch: createPrPrefetch(invokeFn, 5) };
  }

  it("fires one debounced prefetch per PR, deduping repeat hovers", async () => {
    const { calls, prefetch } = recorder();
    prefetch.schedulePrPrefetch("p1", pr());
    prefetch.schedulePrPrefetch("p1", pr());
    expect(calls).toHaveLength(0);
    await wait(20);
    expect(calls).toEqual([["prefetch_pr_open", { projectId: "p1", prNumber: 7, hint: buildPrHint(pr()) }]]);
  });

  it("skips a PR with no head oid or base ref", async () => {
    const { calls, prefetch } = recorder();
    prefetch.schedulePrPrefetch("p1", pr({ head_oid: "" }));
    await wait(20);
    expect(calls).toHaveLength(0);
  });

  it("prefetches remote PRs by owner/repo and skips unparseable remotes", async () => {
    const { calls, prefetch } = recorder();
    prefetch.scheduleRemotePrPrefetch({ id: "p1", remote: "a/b" }, pr());
    prefetch.scheduleRemotePrPrefetch({ id: "p2", remote: null }, pr());
    await wait(20);
    expect(calls).toEqual([["prefetch_remote_pr_open", { owner: "a", repo: "b", number: 7 }]]);
  });

  it("cancel clears both the local and the remote timer for that PR", async () => {
    const { calls, prefetch } = recorder();
    prefetch.schedulePrPrefetch("p1", pr());
    prefetch.scheduleRemotePrPrefetch({ id: "p1", remote: "a/b" }, pr());
    prefetch.cancelPrPrefetch("p1", 7);
    await wait(20);
    expect(calls).toHaveLength(0);
    // A cancelled key can be scheduled again.
    prefetch.schedulePrPrefetch("p1", pr());
    await wait(20);
    expect(calls).toHaveLength(1);
  });

  it("swallows a failed prefetch", async () => {
    let count = 0;
    const prefetch = createPrPrefetch(() => {
      count += 1;
      return Promise.reject(new Error("offline"));
    }, 5);
    prefetch.schedulePrPrefetch("p1", pr());
    await wait(20);
    expect(count).toBe(1);
  });
});
