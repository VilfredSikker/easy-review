import { describe, expect, it } from "bun:test";
import type { PrRow } from "$lib/protocol/generated/PrRow";
import type { Prs } from "$lib/protocol/generated/Prs";
import { PR_CACHE_TTL_MS, detectPrNumber, prCacheAge, projectPrFields } from "./prsAdapter";

function row(number: number, o: Partial<PrRow> = {}): PrRow {
  return {
    number,
    title: `PR ${number}`,
    headRef: `branch-${number}`,
    baseRef: "main",
    headOid: `${number}`.padStart(40, "0"),
    state: "OPEN",
    isDraft: false,
    author: "me",
    assignees: [],
    reviewers: [],
    checksState: null,
    reviewDecision: null,
    mergedAt: null,
    updatedAt: "",
    approvedByMe: false,
    ...o,
  };
}

function prs(rows: PrRow[], o: Partial<Prs> = {}): Prs {
  return { rows, fetchedAtMs: 0, projects: [], ...o };
}

const project = { id: "p", remote: "acme/a", remote_only: false };

describe("PR cache age", () => {
  it("ages a fetched list and marks it stale past ten minutes", () => {
    expect(prCacheAge(project, 1_000, 1_000 + PR_CACHE_TTL_MS)).toEqual({
      pr_cache_stale: false,
      pr_cache_age_ms: PR_CACHE_TTL_MS,
    });
    expect(prCacheAge(project, 1_000, 1_001 + PR_CACHE_TTL_MS)).toEqual({
      pr_cache_stale: true,
      pr_cache_age_ms: PR_CACHE_TTL_MS + 1,
    });
  });

  it("calls a remote never fetched stale, and a project with no remote or no checkout fresh", () => {
    expect(prCacheAge(project, null, 5_000)).toEqual({ pr_cache_stale: true, pr_cache_age_ms: null });
    expect(prCacheAge({ remote: null }, null, 5_000)).toEqual({ pr_cache_stale: false, pr_cache_age_ms: null });
    expect(prCacheAge({ remote: "acme/a", remote_only: true }, null, 5_000)).toEqual({
      pr_cache_stale: false,
      pr_cache_age_ms: null,
    });
  });
});

describe("a project's PR lists", () => {
  it("resolves each list's numbers to rows, in the server's order", () => {
    const value = prs([row(1), row(2), row(3, { state: "MERGED" }), row(4, { title: "Saved", state: "" })], {
      fetchedAtMs: 100,
      projects: [
        { project: "other", mine: [2], toReview: [], recentlyMerged: [], saved: [], recent: [], dismissed: [] },
        { project: "p", mine: [1], toReview: [2], recentlyMerged: [3], saved: [4, 1], recent: [2], dismissed: [7] },
      ],
    });
    const fields = projectPrFields(project, value, 200);
    const numbers = (list: { number: number }[]) => list.map((p) => p.number);
    expect(numbers(fields.my_prs)).toEqual([1]);
    expect(numbers(fields.prs_to_review)).toEqual([2]);
    expect(numbers(fields.recently_merged)).toEqual([3]);
    expect(numbers(fields.saved_prs)).toEqual([4, 1]);
    expect(numbers(fields.recent_prs)).toEqual([2]);
    expect(fields.dismissed_prs).toEqual([7]);
    expect(fields.saved_prs[0]).toMatchObject({ title: "Saved", head_ref: "branch-4", state: "" });
    expect(fields.my_prs[0].head_oid).toBe("1".padStart(40, "0"));
    expect([fields.pr_cache_stale, fields.pr_cache_age_ms]).toEqual([false, 100]);
  });

  it("reads empty until the remote's value arrives", () => {
    const fields = projectPrFields(project, undefined, 200);
    expect([fields.my_prs, fields.saved_prs, fields.dismissed_prs]).toEqual([[], [], []]);
    expect([fields.pr_cache_stale, fields.pr_cache_age_ms]).toEqual([false, null]);
  });
});

/** `snapshot.rs`'s `detected_pr_number`, case by case. */
describe("the active branch's PR", () => {
  const local = { kind: "working" as const, pr_number: null };

  it("takes the tab's own PR first", () => {
    expect(detectPrNumber({ kind: "local_branch", pr_number: 8 }, "feat", [prs([row(1, { headRef: "feat" })])])).toBe(8);
  });

  it("finds none for a remote PR tab without a number", () => {
    expect(detectPrNumber({ kind: "remote_pr", pr_number: null }, "feat", [prs([row(1, { headRef: "feat" })])])).toBeNull();
  });

  it("prefers an open PR on the branch over an earlier closed one, across remotes", () => {
    const closed = prs([row(1, { headRef: "feat", state: "MERGED" })]);
    const open = prs([row(2, { headRef: "feat", state: "OPEN" })]);
    expect(detectPrNumber(local, "feat", [closed, open])).toBe(2);
  });

  it("falls back to the first PR on the branch in any state, dismissed ones included", () => {
    const value = prs([row(3, { headRef: "feat", state: "CLOSED" }), row(4, { headRef: "feat", state: "MERGED" })], {
      projects: [{ project: "p", mine: [], toReview: [], recentlyMerged: [], saved: [], recent: [], dismissed: [3] }],
    });
    expect(detectPrNumber(local, "feat", [value])).toBe(3);
    expect(detectPrNumber(local, "other", [value])).toBeNull();
  });

  it("never matches a title-only row on an empty branch", () => {
    const value = prs([row(5, { headRef: "", state: "" })]);
    expect(detectPrNumber(local, "", [value])).toBeNull();
    expect(detectPrNumber(local, null, [value])).toBeNull();
  });
});
