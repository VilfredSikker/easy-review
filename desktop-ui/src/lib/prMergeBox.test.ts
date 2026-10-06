import { describe, expect, it } from "bun:test";
import type { GithubStatusSnapshot } from "$lib/types";
import {
  allowedMethods,
  canWrite,
  confirmLabel,
  mergeBoxModel,
  mergeButtonLabel,
  mergeHeadNote,
  mergeRequest,
  pickMethod,
  prTarget,
} from "./prMergeBox";

function gh(overrides: Partial<GithubStatusSnapshot> = {}): GithubStatusSnapshot {
  return {
    owner: "o",
    repo: "r",
    number: 12,
    url: "https://github.com/o/r/pull/12",
    state: "OPEN",
    is_draft: false,
    title: "T",
    body: "",
    author: "alice",
    head_ref: "feat/x",
    base_ref: "main",
    review_decision: "APPROVED",
    mergeable: "MERGEABLE",
    labels: [],
    checks: [],
    comments_count: 0,
    reviews_count: 0,
    recent_comments: [],
    recent_reviews: [],
    last_updated: null,
    is_authored_by_me: false,
    merge_state_status: "CLEAN",
    head_oid: "abc123",
    repo_merge: {
      merge_commit_allowed: true,
      squash_merge_allowed: true,
      rebase_merge_allowed: true,
      delete_branch_on_merge: false,
      viewer_permission: "WRITE",
    },
    ...overrides,
  };
}

const kinds = (items: { action: { kind: string } }[]) => items.map((i) => i.action.kind);

describe("mergeBoxModel — open PR", () => {
  it("is ready to merge when GitHub reports CLEAN", () => {
    const m = mergeBoxModel(gh());
    expect(m.tone).toBe("ok");
    expect(m.headline).toBe("Ready to merge");
    expect(m.merge).toEqual({ enabled: true, auto: false, reason: null });
    expect(m.updateProminent).toBe(false);
  });

  it("puts Update branch forward and blocks the merge when BEHIND", () => {
    const m = mergeBoxModel(gh({ merge_state_status: "BEHIND" }));
    expect(m.tone).toBe("warn");
    expect(m.updateProminent).toBe(true);
    expect(m.merge?.enabled).toBe(false);
    expect(m.merge?.reason).toBe("Update the branch first");
    expect(m.detail).toContain("main");
  });

  it("blocks the merge on conflicts", () => {
    const m = mergeBoxModel(gh({ merge_state_status: "DIRTY" }));
    expect(m.tone).toBe("danger");
    expect(m.merge?.enabled).toBe(false);
  });

  it("falls back to `mergeable` when the merge state is missing", () => {
    const m = mergeBoxModel(gh({ merge_state_status: null, mergeable: "CONFLICTING" }));
    expect(m.tone).toBe("danger");
  });

  it("offers auto-merge when blocked only on repos that enabled it", () => {
    const repo = { ...gh().repo_merge!, auto_merge_allowed: true };
    const on = mergeBoxModel(gh({ merge_state_status: "BLOCKED", repo_merge: repo }));
    expect(on.merge).toEqual({ enabled: true, auto: true, reason: null });

    // Off is GitHub's default; the button would be refused.
    const off = mergeBoxModel(gh({ merge_state_status: "BLOCKED" }));
    expect(off.merge).toEqual({
      enabled: false,
      auto: false,
      reason: "Auto-merge is not enabled for this repository",
    });
    expect(mergeBoxModel(gh({ merge_state_status: "BLOCKED", repo_merge: null })).merge?.enabled).toBe(false);
  });

  it("names why a merge is blocked", () => {
    const review = mergeBoxModel(gh({ merge_state_status: "BLOCKED", review_decision: "REVIEW_REQUIRED" }));
    expect(review.detail).toBe("Review required");

    const checks = mergeBoxModel(
      gh({
        merge_state_status: "BLOCKED",
        review_decision: "APPROVED",
        checks: [{ name: "ci", status: "COMPLETED", conclusion: "FAILURE", url: null }],
      }),
    );
    expect(checks.detail).toBe("1 check failing");

    const pending = mergeBoxModel(
      gh({
        merge_state_status: "BLOCKED",
        review_decision: null,
        checks: [{ name: "ci", status: "PENDING", conclusion: "", url: null }],
      }),
    );
    expect(pending.detail).toBe("Checks have not finished");
  });

  it("allows merging past non-required failures with a warning", () => {
    const m = mergeBoxModel(
      gh({
        merge_state_status: "UNSTABLE",
        checks: [{ name: "ci", status: "COMPLETED", conclusion: "FAILURE", url: null }],
      }),
    );
    expect(m.tone).toBe("warn");
    expect(m.merge?.enabled).toBe(true);
    expect(m.detail).toBe("1 failing");
  });

  it("disables the merge while GitHub is still computing", () => {
    for (const status of ["UNKNOWN", null]) {
      const m = mergeBoxModel(gh({ merge_state_status: status, mergeable: "UNKNOWN" }));
      expect(m.merge?.enabled).toBe(false);
      expect(m.tone).toBe("muted");
    }
  });

  it("swaps the merge button for Ready for review on a draft", () => {
    const m = mergeBoxModel(gh({ is_draft: true, merge_state_status: "DRAFT" }));
    expect(m.merge).toBeNull();
    expect(m.readyPrimary).toBe(true);
    expect(kinds(m.menu)).toContain("mark_ready");
    expect(kinds(m.menu)).not.toContain("convert_to_draft");
  });

  it("lists update, draft and close in the menu, with close asking twice", () => {
    const m = mergeBoxModel(gh());
    expect(kinds(m.menu)).toEqual(["update_branch", "update_branch", "convert_to_draft", "close"]);
    expect(m.menu.find((i) => i.action.kind === "close")?.destructive).toBe(true);
    expect(m.menu.find((i) => i.action.kind === "convert_to_draft")?.destructive).toBe(false);
  });

  it("shows a pending auto-merge and offers to disable it", () => {
    const m = mergeBoxModel(gh({ auto_merge_method: "SQUASH", merge_state_status: "BLOCKED" }));
    expect(m.headline).toBe("Auto-merge enabled");
    expect(m.autoMerge).toBe("squash");
    expect(m.merge).toBeNull();
    // Disable is the box's own button; the menu does not repeat it.
    expect(kinds(m.menu)).not.toContain("disable_auto_merge");
  });

  it("offers no method or delete on a merge-queue base, where both are ignored", () => {
    // The queue picks the method, and a delete would drop the PR from the queue.
    const g = gh({ base_has_merge_queue: true });
    const m = mergeBoxModel(g);
    expect(m.mergeQueue).toBe(true);
    expect(m.methods).toEqual([]);
    expect(m.offerDeleteOnMerge).toBe(false);
    expect(m.merge?.enabled).toBe(true);
    expect(mergeButtonLabel("rebase", false, true)).toBe("Add to merge queue");
    expect(confirmLabel(mergeRequest(g, "rebase", false, false), g)).toBe("Add #12 to the main merge queue?");
  });

  it("shows a queued PR without offering Merge again", () => {
    // On a merge-queue branch, merging only enqueues and the PR stays open.
    const m = mergeBoxModel(gh({ in_merge_queue: true }));
    expect(m.headline).toBe("Queued to merge");
    expect(m.merge).toBeNull();
    expect(kinds(m.menu)).toContain("close");
  });

  it("hides the delete-on-merge option when the repo deletes branches itself", () => {
    // A second delete after GitHub's own would fail with "Reference does not exist".
    const repo = { ...gh().repo_merge!, delete_branch_on_merge: true };
    expect(mergeBoxModel(gh({ repo_merge: repo })).offerDeleteOnMerge).toBe(false);
    expect(mergeBoxModel(gh()).offerDeleteOnMerge).toBe(true);
  });

  it("does not offer delete-on-merge for a fork PR", () => {
    expect(mergeBoxModel(gh({ is_cross_repository: true })).offerDeleteOnMerge).toBe(false);
  });

  it("hides every write action without write access", () => {
    const repo = { ...gh().repo_merge!, viewer_permission: "READ" };
    const m = mergeBoxModel(gh({ repo_merge: repo, merge_state_status: "BEHIND" }));
    expect(m.canWrite).toBe(false);
    expect(m.menu).toEqual([]);
    expect(m.updateProminent).toBe(false);
    expect(m.offerDeleteOnMerge).toBe(false);
  });
});

describe("mergeBoxModel — merged or closed PR", () => {
  it("offers Delete branch after a merge while the branch exists", () => {
    const m = mergeBoxModel(gh({ state: "MERGED", head_branch_exists: true }));
    expect(m.phase).toBe("merged");
    expect(m.merge).toBeNull();
    expect(m.menu).toEqual([]);
    expect(kinds(m.primaryActions)).toEqual(["delete_branch"]);
    expect(m.primaryActions[0].destructive).toBe(true);
    expect(m.detail).toBe("Branch feat/x is still on GitHub");
  });

  it("offers Restore branch once it is gone", () => {
    const m = mergeBoxModel(gh({ state: "MERGED", head_branch_exists: false }));
    expect(kinds(m.primaryActions)).toEqual(["restore_branch"]);
    expect(m.detail).toBe("Branch feat/x was deleted");
  });

  it("cannot restore without the head commit", () => {
    const m = mergeBoxModel(gh({ state: "MERGED", head_branch_exists: false, head_oid: "" }));
    expect(m.primaryActions).toEqual([]);
  });

  it("offers nothing for the branch when its existence is unknown", () => {
    const m = mergeBoxModel(gh({ state: "MERGED", head_branch_exists: null }));
    expect(m.primaryActions).toEqual([]);
    expect(m.detail).toBeNull();
  });

  it("offers Reopen and the branch action on a closed PR", () => {
    const m = mergeBoxModel(gh({ state: "CLOSED", head_branch_exists: true }));
    expect(m.phase).toBe("closed");
    expect(m.tone).toBe("danger");
    expect(kinds(m.primaryActions)).toEqual(["reopen", "delete_branch"]);
  });

  it("does not offer Reopen once the branch is gone", () => {
    // GitHub refuses to reopen a PR without its head branch.
    const m = mergeBoxModel(gh({ state: "CLOSED", head_branch_exists: false }));
    expect(kinds(m.primaryActions)).toEqual(["restore_branch"]);
  });

  it("offers no branch action for a fork PR", () => {
    // The branch lives in the fork; a same-named branch in the base repo is
    // someone else's.
    const merged = mergeBoxModel(gh({ state: "MERGED", is_cross_repository: true, head_branch_exists: true }));
    expect(merged.primaryActions).toEqual([]);
    // The status fetch never looks a fork's branch up, so existence is unknown.
    const closed = mergeBoxModel(gh({ state: "CLOSED", is_cross_repository: true, head_branch_exists: null }));
    expect(kinds(closed.primaryActions)).toEqual(["reopen"]);
  });

  it("offers nothing without write access", () => {
    const repo = { ...gh().repo_merge!, viewer_permission: "TRIAGE" };
    expect(mergeBoxModel(gh({ state: "CLOSED", head_branch_exists: true, repo_merge: repo })).primaryActions).toEqual([]);
  });
});

describe("merge methods", () => {
  it("lists only what the repo allows, in GitHub's order", () => {
    const repo = { ...gh().repo_merge!, merge_commit_allowed: false };
    expect(allowedMethods(gh({ repo_merge: repo }))).toEqual(["squash", "rebase"]);
  });

  it("offers every method when settings are unknown", () => {
    expect(allowedMethods(gh({ repo_merge: null }))).toEqual(["merge", "squash", "rebase"]);
  });

  it("remembers the last method only while it is allowed", () => {
    expect(pickMethod(["merge", "squash"], "squash")).toBe("squash");
    expect(pickMethod(["merge", "squash"], "rebase")).toBe("merge");
    expect(pickMethod(["rebase"], null)).toBe("rebase");
  });

  it("labels the button like GitHub", () => {
    expect(mergeButtonLabel("merge", false)).toBe("Merge pull request");
    expect(mergeButtonLabel("squash", false)).toBe("Squash and merge");
    expect(mergeButtonLabel("rebase", true)).toBe("Enable auto-merge (rebase)");
  });

  it("treats unknown permission as writable", () => {
    expect(canWrite(gh({ repo_merge: null }))).toBe(true);
    expect(canWrite(gh({ repo_merge: { ...gh().repo_merge!, viewer_permission: "MAINTAIN" } }))).toBe(true);
  });
});

describe("mergeRequest", () => {
  it("pins the head the card showed", () => {
    expect(mergeRequest(gh(), "squash", false, true)).toEqual({
      kind: "merge",
      method: "squash",
      auto: false,
      expected_head: "abc123",
      delete_branch: true,
    });
  });

  it("pins the reviewed PR diff's head when one is on screen", () => {
    // A push after the review must make GitHub refuse the merge (ADR 0040).
    const req = mergeRequest(gh({ head_oid: "pushed-later" }), "merge", false, false, "reviewed");
    expect(req.kind === "merge" && req.expected_head).toBe("reviewed");
  });

  it("never deletes the branch for an auto-merge, which has not happened yet", () => {
    const req = mergeRequest(gh(), "merge", true, true);
    expect(req.kind === "merge" && req.delete_branch).toBe(false);
  });
});

describe("mergeHeadNote", () => {
  it("says nothing when the PR diff on screen is the PR head", () => {
    expect(mergeHeadNote(gh(), "abc123")).toBeNull();
  });

  it("blocks the merge when the PR head and the diff on screen differ", () => {
    const note = mergeHeadNote(gh({ head_oid: "def4567890" }), "abc1234");
    expect(note?.warn).toBe(true);
    // GitHub would refuse the merge, pinned as it is to the reviewed head.
    expect(note?.blocksMerge).toBe(true);
    expect(note?.text).toContain("def4567");
    expect(note?.text).toContain("abc1234");
    // Right after a Sync the diff is the newer side, so the text must not
    // claim the PR moved or that syncing again is the only fix.
    expect(note?.text).not.toContain("moved");
    expect(note?.text).toContain("wait for the status to refresh");
  });

  it("says what lands when the view is the local branch", () => {
    const note = mergeHeadNote(gh(), null);
    expect(note).toEqual({
      text: "This view shows your local branch. The merge lands PR head abc123.",
      warn: false,
      blocksMerge: false,
    });
  });

  it("says nothing without a PR head", () => {
    expect(mergeHeadNote(gh({ head_oid: "" }), "abc123")).toBeNull();
  });
});

describe("prTarget", () => {
  it("names the PR by owner, repo and number", () => {
    expect(prTarget(gh())).toEqual({ owner: "o", repo: "r", number: 12 });
  });
});

describe("confirmLabel", () => {
  it("names the PR, method and target", () => {
    const g = gh();
    expect(confirmLabel(mergeRequest(g, "squash", false, false), g)).toBe("Squash and merge #12 into main?");
    expect(confirmLabel({ kind: "close" }, g)).toBe("Close #12 without merging?");
    expect(confirmLabel({ kind: "delete_branch" }, g)).toBe("Delete feat/x on GitHub?");
  });
});
