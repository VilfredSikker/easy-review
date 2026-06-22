import { describe, expect, it } from "vitest";
import { reviewScopeFromMode, scopeDescriptionFromMode } from "./reviewScope";

describe("reviewScopeFromMode", () => {
  it("maps pr and branch to branch scope", () => {
    expect(reviewScopeFromMode("pr")).toBe("branch");
    expect(reviewScopeFromMode("branch")).toBe("branch");
  });

  it("maps guide (tour) to branch scope", () => {
    expect(reviewScopeFromMode("tour")).toBe("branch");
  });

  it("maps working-tree modes", () => {
    expect(reviewScopeFromMode("unstaged")).toBe("unstaged");
    expect(reviewScopeFromMode("staged")).toBe("staged");
  });

  it("returns null for unsupported modes", () => {
    expect(reviewScopeFromMode("history")).toBeNull();
    expect(reviewScopeFromMode(undefined)).toBeNull();
  });
});

describe("scopeDescriptionFromMode", () => {
  it("describes PR diff mode", () => {
    expect(scopeDescriptionFromMode("pr")).toBe("PR diff vs base");
  });

  it("describes guide (tour) as the branch change set", () => {
    expect(scopeDescriptionFromMode("tour")).toBe("All changes vs base");
  });
});
