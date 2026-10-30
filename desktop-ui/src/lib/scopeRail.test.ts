import { describe, expect, it } from "bun:test";
import {
  commitDiffstat,
  footerScopeLabel,
  localScopes,
  pressedScope,
  viewIsPr,
} from "./scopeRail";

describe("pressedScope", () => {
  it("lights the working-tree scopes as themselves", () => {
    expect(pressedScope("unstaged")).toBe("unstaged");
    expect(pressedScope("staged")).toBe("staged");
  });

  it("keeps Branch lit in a commit and in the Tour", () => {
    expect(pressedScope("branch")).toBe("branch");
    expect(pressedScope("history")).toBe("branch");
    expect(pressedScope("tour")).toBe("branch");
  });

  it("lights nothing in the PR diff view", () => {
    expect(pressedScope("pr")).toBeNull();
    expect(pressedScope("conflicts")).toBeNull();
  });
});

const all = { viewBranch: true, viewUnstaged: true, viewStaged: true };

describe("localScopes", () => {
  it("offers all three on a checked-out local tab", () => {
    expect(localScopes({ remote: false, readOnly: false, features: all })).toEqual([
      "branch",
      "unstaged",
      "staged",
    ]);
  });

  it("offers none on a remote PR tab", () => {
    expect(localScopes({ remote: true, readOnly: true, features: all })).toEqual([]);
  });

  it("drops the working-tree scopes on a branch that is not checked out", () => {
    expect(localScopes({ remote: false, readOnly: true, features: all })).toEqual(["branch"]);
  });

  it("respects disabled views", () => {
    expect(
      localScopes({
        remote: false,
        readOnly: false,
        features: { viewBranch: true, viewUnstaged: false, viewStaged: true },
      }),
    ).toEqual(["branch", "staged"]);
  });
});

describe("viewIsPr", () => {
  it("follows the mode outside History", () => {
    expect(viewIsPr("pr", undefined, false, false)).toBe(true);
    expect(viewIsPr("branch", undefined, false, true)).toBe(false);
    expect(viewIsPr("unstaged", undefined, false, true)).toBe(false);
  });

  it("follows the tour's scope in Tour", () => {
    expect(viewIsPr("tour", "pr", false, false)).toBe(true);
    expect(viewIsPr("tour", "branch", false, true)).toBe(false);
  });

  it("inherits the remembered source in History", () => {
    expect(viewIsPr("history", undefined, false, true)).toBe(true);
    expect(viewIsPr("history", undefined, false, false)).toBe(false);
  });

  it("is always PR on a remote tab", () => {
    expect(viewIsPr("history", undefined, true, false)).toBe(true);
    expect(viewIsPr("branch", undefined, true, false)).toBe(true);
  });
});

describe("footerScopeLabel", () => {
  it("names the view outside History", () => {
    expect(footerScopeLabel("branch", false, null)).toBe("Local branch");
    expect(footerScopeLabel("pr", true, null)).toBe("PR Diff");
    expect(footerScopeLabel("staged", false, null)).toBe("Staged");
    expect(footerScopeLabel("tour", true, null)).toBe("Tour · PR Diff");
  });

  it("names the commit in History, whose marks are its own bucket", () => {
    expect(footerScopeLabel("history", true, "2afab9e0c4")).toBe("Commit 2afab9e");
    expect(footerScopeLabel("history", false, null)).toBe("Commit");
  });
});

describe("commitDiffstat", () => {
  it("returns known counts, zeros included", () => {
    expect(commitDiffstat({ additions: 0, deletions: 0 })).toEqual({ additions: 0, deletions: 0 });
    expect(commitDiffstat({ additions: 6, deletions: 4 })).toEqual({ additions: 6, deletions: 4 });
  });

  it("is null when either side is unknown", () => {
    expect(commitDiffstat({ additions: null, deletions: 3 })).toBeNull();
    expect(commitDiffstat({})).toBeNull();
  });
});
