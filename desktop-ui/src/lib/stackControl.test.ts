import { describe, expect, it } from "bun:test";
import {
  shouldShowStackControl,
  stackBadge,
  stackRowTitle,
  stackRows,
  stackSummary,
  stackTitle,
  stackUnknown,
} from "./stackControl";
import type { StackLayerSnapshot, StackSnapshot } from "./types";

function layer(over: Partial<StackLayerSnapshot> = {}): StackLayerSnapshot {
  return {
    branch: "feat/auth",
    pr_number: 41,
    pr_url: "https://github.com/o/r/pull/41",
    state: "open",
    is_current: false,
    enabled: true,
    needs_rebase: false,
    ...over,
  };
}

function stack(over: Partial<StackSnapshot> = {}): StackSnapshot {
  const layers = over.layers ?? [
    layer({ branch: "feat/ui", pr_number: 43, state: "open" }),
    layer({
      branch: "feat/api",
      pr_number: 42,
      state: "needs rebase",
      needs_rebase: true,
      is_current: true,
    }),
    layer({ branch: "feat/auth", pr_number: 41, state: "merged" }),
  ];
  return {
    trunk: "main",
    layers,
    // Top-of-stack first, so `feat/api` sits at position 2 of 3.
    position: 2,
    size: layers.length,
    unavailable: null,
    retryable: false,
    loading: false,
    ...over,
  };
}

describe("stackRows", () => {
  it("keeps top-of-stack order and appends the trunk", () => {
    const rows = stackRows(stack());
    expect(rows.map((r) => r.branch)).toEqual([
      "feat/ui",
      "feat/api",
      "feat/auth",
      "main",
    ]);
  });

  it("marks exactly the current layer, never the trunk", () => {
    const rows = stackRows(stack());
    expect(rows.filter((r) => r.is_current).map((r) => r.branch)).toEqual([
      "feat/api",
    ]);
  });

  it("labels the PR reference and state per row", () => {
    const rows = stackRows(stack());
    expect(rows[0].pr_ref).toBe("#43");
    expect(rows[1].state).toBe("needs rebase");
    expect(rows[1].needs_rebase).toBe(true);
    expect(rows[3]).toMatchObject({
      branch: "main",
      pr_ref: "",
      state: "trunk",
      pr_number: null,
    });
  });

  it("only lets you switch to another layer that has a PR", () => {
    const rows = stackRows(
      stack({
        layers: [
          layer({ branch: "feat/new", pr_number: null, enabled: false, state: "no PR yet" }),
          layer({ branch: "feat/api", pr_number: 42, is_current: true }),
          layer({ branch: "feat/auth", pr_number: 41 }),
        ],
        position: 2,
        size: 3,
      }),
    );
    // No PR yet — nothing to open.
    expect(rows[0].selectable).toBe(false);
    expect(rows[0].pr_ref).toBe("");
    // Currently viewing — not "another" layer.
    expect(rows[1].selectable).toBe(false);
    expect(rows[2].selectable).toBe(true);
    // The trunk is never selectable.
    expect(rows[3].selectable).toBe(false);
  });

  it("describes each row's action in its tooltip", () => {
    const rows = stackRows(stack());
    expect(stackRowTitle(rows[0])).toBe("Review PR #43 (feat/ui)");
    expect(stackRowTitle(rows[1])).toBe("Currently viewing");
    expect(stackRowTitle(rows[3])).toBe("Trunk the stack is based on");

    const noPr = stackRows(
      stack({
        layers: [layer({ branch: "feat/new", pr_number: null, enabled: false })],
        position: 1,
        size: 1,
      }),
    );
    expect(stackRowTitle(noPr[0])).toBe("No PR yet");
  });

  it("returns no rows for an unavailable stack or no stack at all", () => {
    expect(stackRows(null)).toEqual([]);
    expect(stackRows(undefined)).toEqual([]);
    expect(
      stackRows(stack({ layers: [], size: 0, position: null, unavailable: "not in a stack" })),
    ).toEqual([]);
  });

  it("omits the trunk row when the payload has no trunk", () => {
    const rows = stackRows(stack({ trunk: "" }));
    expect(rows.map((r) => r.branch)).toEqual(["feat/ui", "feat/api", "feat/auth"]);
  });

  it("does not repeat the trunk when the payload already lists it", () => {
    const rows = stackRows(
      stack({
        trunk: "main",
        layers: [layer({ branch: "main", pr_number: null, enabled: false, state: "trunk" })],
      }),
    );
    expect(rows.map((r) => r.branch)).toEqual(["main"]);
  });
});

describe("stackBadge", () => {
  it("counts layers, not rows, so the trunk is excluded", () => {
    const badge = stackBadge(stack());
    expect(badge).toBe("2 / 3");
    expect(stackRows(stack())).toHaveLength(4);
  });

  it("is null without a position or a stack", () => {
    expect(stackBadge(null)).toBeNull();
    expect(stackBadge(stack({ position: null }))).toBeNull();
    expect(stackBadge(stack({ layers: [], size: 0, position: null }))).toBeNull();
  });
});

describe("stackTitle", () => {
  it("names the viewed layer", () => {
    expect(stackTitle(stack())).toBe("Stack layer 2 / 3 — viewing feat/api");
  });

  it("surfaces the reason when there is no stack", () => {
    expect(
      stackTitle(stack({ layers: [], size: 0, position: null, unavailable: "not in a stack" })),
    ).toBe("Stacked PRs — not in a stack");
  });
});

describe("stackSummary", () => {
  it("shows the badge when there is a position", () => {
    expect(stackSummary(stack())).toBe("2 / 3");
  });

  it("shows a pending label while loading", () => {
    expect(stackSummary(stack({ loading: true }))).toBe("Reading…");
  });

  it("falls back to a quiet label before a lookup lands or without a position", () => {
    expect(stackSummary(stack({ position: null }))).toBe("Stack");
    expect(
      stackSummary(stack({ layers: [], size: 0, position: null, loading: false })),
    ).toBe("Stack");
  });

  it("is null only with no stack at all", () => {
    expect(stackSummary(null)).toBeNull();
  });
});

describe("stackUnknown", () => {
  it("is true before a lookup lands", () => {
    expect(stackUnknown(null)).toBe(true);
    expect(stackUnknown(stack({ layers: [], size: 0, position: null }))).toBe(true);
  });

  it("is false once a lookup has an answer", () => {
    expect(stackUnknown(stack())).toBe(false);
    expect(
      stackUnknown(stack({ layers: [], size: 0, position: null, unavailable: "nope" })),
    ).toBe(false);
  });
});

describe("shouldShowStackControl", () => {
  it("shows the control for a real stack", () => {
    expect(shouldShowStackControl(stack())).toBe(true);
  });

  it("offers the control on a PR branch before the first lookup lands", () => {
    const unknown = stack({ layers: [], size: 0, position: null });
    expect(shouldShowStackControl(unknown, true)).toBe(true);
    // A plain branch (no PR) keeps its header quiet until something is known.
    expect(shouldShowStackControl(unknown, false)).toBe(false);
  });

  it("shows it while the first lookup is in flight", () => {
    expect(
      shouldShowStackControl(stack({ layers: [], size: 0, position: null, loading: true })),
    ).toBe(true);
  });

  it("stays hidden when there is no data or a definitive reason not to show one", () => {
    expect(shouldShowStackControl(null)).toBe(false);
    expect(
      shouldShowStackControl(stack({ layers: [], size: 0, position: null, unavailable: "nope" })),
    ).toBe(false);
  });

  it("keeps a failed lookup reachable so it can be retried", () => {
    const failed = stack({
      layers: [],
      size: 0,
      position: null,
      unavailable: "Failed to run `gh stack view`",
      retryable: true,
    });
    expect(shouldShowStackControl(failed)).toBe(true);
    expect(shouldShowStackControl(failed, false)).toBe(true);
    expect(stackSummary(failed)).toBe("Stack");
  });
});
