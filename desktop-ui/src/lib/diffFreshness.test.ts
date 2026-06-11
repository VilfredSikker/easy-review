import { describe, expect, test } from "bun:test";
import {
  DIFF_STALE_AFTER_MS,
  diffFreshState,
  diffFreshTooltip,
  relativeSyncTime,
  shortOid,
} from "./diffFreshness";

const NOW = 1_750_000_000_000;

describe("diffFreshState", () => {
  test("refreshing wins over everything (single indicator)", () => {
    expect(
      diffFreshState({ refreshing: true, outdated: true, syncedAtMs: null, nowMs: NOW })
    ).toBe("refreshing");
    expect(
      diffFreshState({ refreshing: true, outdated: false, syncedAtMs: NOW, nowMs: NOW })
    ).toBe("refreshing");
  });

  test("head mismatch is stale even when recently synced", () => {
    expect(
      diffFreshState({ refreshing: false, outdated: true, syncedAtMs: NOW - 1000, nowMs: NOW })
    ).toBe("stale");
  });

  test("never-confirmed sync is stale", () => {
    expect(
      diffFreshState({ refreshing: false, outdated: false, syncedAtMs: null, nowMs: NOW })
    ).toBe("stale");
  });

  test("sync older than threshold is stale; within threshold is fresh", () => {
    expect(
      diffFreshState({
        refreshing: false,
        outdated: false,
        syncedAtMs: NOW - DIFF_STALE_AFTER_MS - 1,
        nowMs: NOW,
      })
    ).toBe("stale");
    expect(
      diffFreshState({
        refreshing: false,
        outdated: false,
        syncedAtMs: NOW - DIFF_STALE_AFTER_MS + 1000,
        nowMs: NOW,
      })
    ).toBe("fresh");
  });
});

describe("relativeSyncTime", () => {
  test("buckets", () => {
    expect(relativeSyncTime(null, NOW)).toBeNull();
    expect(relativeSyncTime(NOW - 30_000, NOW)).toBe("just now");
    expect(relativeSyncTime(NOW - 3 * 60_000, NOW)).toBe("3m ago");
    expect(relativeSyncTime(NOW - 2 * 3_600_000, NOW)).toBe("2h ago");
    expect(relativeSyncTime(NOW - 5 * 86_400_000, NOW)).toBe("5d ago");
  });

  test("clock skew clamps to just now", () => {
    expect(relativeSyncTime(NOW + 60_000, NOW)).toBe("just now");
  });
});

describe("shortOid", () => {
  test("truncates and rejects empties", () => {
    expect(shortOid("abcdef0123456789")).toBe("abcdef0");
    expect(shortOid("")).toBeNull();
    expect(shortOid("   ")).toBeNull();
    expect(shortOid(null)).toBeNull();
    expect(shortOid(undefined)).toBeNull();
  });
});

describe("diffFreshTooltip", () => {
  test("stale tooltip names both heads and the sync age", () => {
    const tip = diffFreshTooltip({
      state: "stale",
      headOid: "aaaaaaa1111",
      latestOid: "bbbbbbb2222",
      syncedAtMs: NOW - 3 * 60_000,
      nowMs: NOW,
    });
    expect(tip).toContain("Diff may be outdated");
    expect(tip).toContain("Rendered head: aaaaaaa");
    expect(tip).toContain("Latest head: bbbbbbb");
    expect(tip).toContain("Last synced 3m ago");
  });

  test("fresh tooltip omits a matching latest head", () => {
    const tip = diffFreshTooltip({
      state: "fresh",
      headOid: "aaaaaaa1111",
      latestOid: "aaaaaaa1111",
      syncedAtMs: NOW - 30_000,
      nowMs: NOW,
    });
    expect(tip).toContain("Diff is up to date");
    expect(tip).toContain("Rendered head: aaaaaaa");
    expect(tip).not.toContain("Latest head");
    expect(tip).toContain("Last synced just now");
  });

  test("unconfirmed sync is spelled out", () => {
    const tip = diffFreshTooltip({
      state: "refreshing",
      headOid: null,
      latestOid: null,
      syncedAtMs: null,
      nowMs: NOW,
    });
    expect(tip).toContain("Refreshing diff in the background");
    expect(tip).toContain("Last sync not confirmed yet");
  });
});
