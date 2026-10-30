import { describe, expect, test } from "bun:test";
import {
  localBranchBannerCopy,
  oidSegments,
  reviewedClearedBySync,
  shortOid,
  staleBannerCopy,
} from "./shellBanners";
import type { DiffStaleSnapshot } from "./types";

const OLD = "7be0d14aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NEW = "9f3e1a2bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PR = "c0ffee1ccccccccccccccccccccccccccccccccc";

function stale(over: Partial<DiffStaleSnapshot> = {}): DiffStaleSnapshot {
  return { kind: "pr_head", message: "", built_from_oid: OLD, latest_oid: NEW, ...over };
}

describe("shortOid", () => {
  test("keeps seven characters", () => {
    expect(shortOid(NEW)).toBe("9f3e1a2");
  });
});

describe("staleBannerCopy", () => {
  test("names both commits and the commit count when it is known", () => {
    const c = staleBannerCopy(stale({ commits_behind: 2 }), "main");
    expect(c.latestShort).toBe("9f3e1a2");
    expect(c.builtShort).toBe("7be0d14");
    expect(c.message).toBe(
      "The PR head moved to 9f3e1a2 (2 new commits). This diff, its triage, findings and tour were " +
        "built from 7be0d14. Syncing clears Reviewed on files whose diff changed.",
    );
  });

  test("uses the singular for one commit", () => {
    expect(staleBannerCopy(stale({ commits_behind: 1 }), null).message).toContain("(1 new commit)");
  });

  // The count needs the new commits locally; it is absent until they are fetched.
  test("leaves the count out when it is unknown", () => {
    expect(staleBannerCopy(stale(), null).message).toStartWith("The PR head moved to 9f3e1a2. ");
  });

  test("names the base when the base moved", () => {
    const c = staleBannerCopy(stale({ kind: "base", commits_behind: 3 }), "origin/main");
    expect(c.message).toStartWith("The base origin/main moved to 9f3e1a2 (3 new commits).");
    expect(c.message).toContain("built against 7be0d14");
  });

  // ADR 0010: the artifacts still match the diff on screen until a Sync.
  test("never calls the artifacts stale", () => {
    expect(staleBannerCopy(stale(), null).message.toLowerCase()).not.toContain("stale");
  });
});

describe("oidSegments", () => {
  test("marks each short SHA as code", () => {
    expect(oidSegments("moved to 9f3e1a2. built from 7be0d14.", ["9f3e1a2", "7be0d14"])).toEqual([
      { text: "moved to ", code: false },
      { text: "9f3e1a2", code: true },
      { text: ". built from ", code: false },
      { text: "7be0d14", code: true },
      { text: ".", code: false },
    ]);
  });

  test("keeps the message whole without SHAs", () => {
    expect(oidSegments("no change", [])).toEqual([{ text: "no change", code: false }]);
  });
});

describe("localBranchBannerCopy", () => {
  test("names unpushed commits and unstaged changes", () => {
    const c = localBranchBannerCopy({ pr_head_oid: PR, ahead: 1, unstaged_files: 1 });
    expect(c).toStartWith("Your worktree is 1 unpushed commit ahead of the PR head and has 1 unstaged change.");
  });

  test("names only what diverges", () => {
    expect(localBranchBannerCopy({ pr_head_oid: PR, ahead: 3, unstaged_files: 0 })).toStartWith(
      "Your worktree is 3 unpushed commits ahead of the PR head.",
    );
    expect(localBranchBannerCopy({ pr_head_oid: PR, ahead: 0, unstaged_files: 2 })).toStartWith(
      "Your worktree has 2 unstaged changes.",
    );
  });

  // ADR 0025: a tour is reused across views when the diffs hash the same.
  test("does not claim the tour stays behind", () => {
    const c = localBranchBannerCopy({ pr_head_oid: PR, ahead: 1, unstaged_files: 0 }) ?? "";
    expect(c).toContain("the tour is shared while both diffs match");
    expect(c).not.toContain("don't carry over");
  });

  test("is null when the worktree matches the PR head", () => {
    expect(localBranchBannerCopy({ pr_head_oid: PR, ahead: 0, unstaged_files: 0 })).toBeNull();
  });

  // A chrome-only snapshot omits the unstaged count; nothing known diverges.
  test("is null when only the unstaged count is missing and nothing is ahead", () => {
    expect(localBranchBannerCopy({ pr_head_oid: PR, ahead: 0 })).toBeNull();
  });

  test("says the worktree may differ when the PR head is not local", () => {
    expect(localBranchBannerCopy({ pr_head_oid: PR, unstaged_files: 0 })).toStartWith(
      "The PR head c0ffee1 is not fetched, so this worktree may differ from it.",
    );
  });
});

describe("reviewedClearedBySync", () => {
  const f = (path: string, reviewed: boolean) => ({ path, reviewed });

  test("counts files that lost their Reviewed mark", () => {
    expect(
      reviewedClearedBySync(
        [f("a", true), f("b", true), f("c", false)],
        [f("a", false), f("b", true), f("c", false)],
      ),
    ).toBe(1);
  });

  test("ignores files that left the diff", () => {
    expect(reviewedClearedBySync([f("a", true)], [f("b", false)])).toBe(0);
  });
});
