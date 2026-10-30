import { describe, expect, test } from "bun:test";
import { approveGate, displayMenuCounts, themeToggleTarget } from "./viewBarModel";
import type { AppSnapshot, FlatFinding, GithubStatusSnapshot, ThreadSnapshot } from "./types";

describe("themeToggleTarget", () => {
  test("switches each dark theme to a light one and back", () => {
    expect(themeToggleTarget("graphite", null)).toBe("daylight");
    expect(themeToggleTarget("daylight", null)).toBe("graphite");
    expect(themeToggleTarget("ember", null)).toBe("paper");
    expect(themeToggleTarget("paper", null)).toBe("ember");
    expect(themeToggleTarget("contrast-dark", null)).toBe("contrast-light");
    expect(themeToggleTarget("contrast-light", null)).toBe("contrast-dark");
  });

  test("returns to the theme it switched away from", () => {
    expect(themeToggleTarget("slate", null)).toBe("daylight");
    expect(themeToggleTarget("daylight", "slate")).toBe("slate");
  });

  // A previous theme on the same side is a pick made elsewhere (Settings),
  // not one this button left.
  test("ignores a previous theme on the same side", () => {
    expect(themeToggleTarget("daylight", "paper")).toBe("graphite");
  });

  test("resolves aliases and unknown names", () => {
    expect(themeToggleTarget("daybreak", null)).toBe("graphite");
    expect(themeToggleTarget("no-such-theme", null)).toBe("daylight");
  });
});

describe("approveGate", () => {
  const gh = (over: Partial<GithubStatusSnapshot> = {}) =>
    ({ state: "OPEN", is_authored_by_me: false, ...over }) as GithubStatusSnapshot;

  test("is hidden without an open PR", () => {
    expect(approveGate(null, false).visible).toBe(false);
    expect(approveGate(gh({ state: "MERGED" }), false).visible).toBe(false);
  });

  test("is disabled on your own PR, with the reason", () => {
    const g = approveGate(gh({ is_authored_by_me: true }), false);
    expect(g).toEqual({ visible: true, enabled: false, reason: "GitHub does not allow approving your own PR" });
  });

  test("is disabled while a submit is in flight", () => {
    expect(approveGate(gh(), true).enabled).toBe(false);
    expect(approveGate(gh(), false).enabled).toBe(true);
  });
});

describe("displayMenuCounts", () => {
  const thread = (id: string, kind: ThreadSnapshot["kind"]) => ({ id, kind }) as ThreadSnapshot;
  const finding = (resolved: boolean) => ({ resolved }) as FlatFinding;

  test("counts hunk comments once alongside the overlay copy", () => {
    const snap = {
      ai: {
        threads: [thread("c1", "comment"), thread("q1", "question"), thread("n1", "note")],
        findings: [finding(false)],
        resolved_findings: [finding(true)],
      },
      files: [{ hunks: [{ threads: [thread("c1", "comment"), thread("c2", "comment")] }] }],
    } as unknown as Pick<AppSnapshot, "ai" | "files">;
    expect(displayMenuCounts(snap)).toEqual({
      comments: 2,
      questions: 1,
      notes: 1,
      findingsUnresolved: 1,
      findingsResolved: 1,
    });
  });

  test("is all zero before a snapshot", () => {
    expect(displayMenuCounts(null).comments).toBe(0);
  });
});
