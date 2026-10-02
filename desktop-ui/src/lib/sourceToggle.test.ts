import { describe, expect, test } from "bun:test";
import { sourceToggleState } from "./sourceToggle";

describe("sourceToggleState", () => {
  test("enables both sides on a working tab with a PR", () => {
    const s = sourceToggleState({ prNumber: 7, tabKind: "working", localBranchCheckedOut: false });
    expect(s.localAvailable).toBe(true);
    expect(s.prAvailable).toBe(true);
  });

  test("disables PR Diff when the branch has no PR", () => {
    const s = sourceToggleState({ prNumber: null, tabKind: "working", localBranchCheckedOut: false });
    expect(s.localAvailable).toBe(true);
    expect(s.prAvailable).toBe(false);
    expect(s.prReason).toBe("No PR for this branch");
  });

  test("disables Local Branch on a remote PR tab", () => {
    const s = sourceToggleState({ prNumber: 7, tabKind: "remote_pr", localBranchCheckedOut: false });
    expect(s.localAvailable).toBe(false);
    expect(s.prAvailable).toBe(true);
    expect(s.localReason).toBe("Remote PR: no local checkout");
  });

  // A branch view with no PR diffs the branch ref against its base, which
  // needs no worktree.
  test("keeps Local Branch on a branch tab without a PR or a checkout", () => {
    const s = sourceToggleState({ prNumber: null, tabKind: "local_branch", localBranchCheckedOut: false });
    expect(s.localAvailable).toBe(true);
  });

  // With a PR and no checkout, Branch mode loads `gh pr diff` — the same diff
  // as PR Diff — so Local has nothing of its own to show.
  test("disables Local Branch on a PR branch tab that is not checked out", () => {
    const s = sourceToggleState({ prNumber: 7, tabKind: "local_branch", localBranchCheckedOut: false });
    expect(s.localAvailable).toBe(false);
    expect(s.prAvailable).toBe(true);
    expect(s.localReason).toBe("Branch is not checked out: same diff as PR Diff");
  });

  test("enables both sides on a PR branch tab that is checked out", () => {
    const s = sourceToggleState({ prNumber: 7, tabKind: "local_branch", localBranchCheckedOut: true });
    expect(s.localAvailable).toBe(true);
    expect(s.prAvailable).toBe(true);
  });
});
