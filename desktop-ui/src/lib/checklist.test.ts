import { describe, expect, it } from "bun:test";
import {
  CHECKLIST_CATEGORY_ORDER,
  checklistCategoryLabel,
  checklistProgress,
  groupChecklistItems,
} from "./checklist";
import type { ChecklistItemSnapshot } from "./types";

function item(overrides: Partial<ChecklistItemSnapshot> = {}): ChecklistItemSnapshot {
  return {
    id: "c-1",
    text: "An outcome worth confirming",
    category: "tests",
    checked: false,
    related_findings: [],
    related_files: [],
    ...overrides,
  };
}

describe("groupChecklistItems", () => {
  it("keeps each item's index in the flat list, whatever the display order", () => {
    // The index is the toggle address on the wire: grouping may move rows, and
    // an index taken from the rendered position would toggle the wrong item.
    const items = [
      item({ id: "plan-1", category: "plan" }),
      item({ id: "schema-1", category: "schema" }),
      item({ id: "tests-1", category: "tests" }),
    ];
    const groups = groupChecklistItems(items);

    expect(groups.map((g) => g.label)).toEqual(["Schema", "Tests", "Plan"]);
    expect(groups[0].rows[0]).toEqual({ index: 1, item: items[1] });
    expect(groups[1].rows[0].index).toBe(2);
    expect(groups[2].rows[0].index).toBe(0);
  });

  it("orders the five outcome categories, then everything else in file order", () => {
    const items = [
      item({ id: "custom-1", category: "correctness" }),
      item({ id: "auth-1", category: "auth" }),
      item({ id: "legacy-1", category: "" }),
      item({ id: "schema-1", category: "schema" }),
      item({ id: "custom-2", category: "correctness" }),
    ];
    const groups = groupChecklistItems(items);

    // schema precedes auth in the canonical order; the two entries with no
    // canonical position follow both, in file order.
    expect(groups.map((g) => g.category)).toEqual(["schema", "auth", "correctness", ""]);
    expect(CHECKLIST_CATEGORY_ORDER.indexOf("schema")).toBeLessThan(
      CHECKLIST_CATEGORY_ORDER.indexOf("auth"),
    );
    // Two items sharing an unknown category stay in one group, in the order the
    // generator wrote them.
    const custom = groups.find((g) => g.category === "correctness")!;
    expect(custom.rows.map((r) => r.item.id)).toEqual(["custom-1", "custom-2"]);
  });

  it("trims a padded category so it groups with the bare one", () => {
    const groups = groupChecklistItems([
      item({ id: "a", category: " tests" }),
      item({ id: "b", category: "tests" }),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0].rows.map((r) => r.item.id)).toEqual(["a", "b"]);
  });
});

describe("checklistProgress", () => {
  it("counts checked items against the whole list", () => {
    expect(
      checklistProgress([
        item({ checked: true }),
        item({ checked: false }),
        item({ checked: true }),
        item({ checked: false }),
        item({ checked: false }),
        item({ checked: false }),
        item({ checked: false }),
      ]),
    ).toBe("2/7");
  });

  it("reports 0/0 for an empty checklist", () => {
    expect(checklistProgress([])).toBe("0/0");
  });
});

describe("checklistCategoryLabel", () => {
  it("names the five outcome categories and passes anything else through", () => {
    expect(checklistCategoryLabel("api")).toBe("API");
    expect(checklistCategoryLabel("plan")).toBe("Plan");
    expect(checklistCategoryLabel("correctness")).toBe("correctness");
    expect(checklistCategoryLabel("")).toBe("Other");
  });
});
