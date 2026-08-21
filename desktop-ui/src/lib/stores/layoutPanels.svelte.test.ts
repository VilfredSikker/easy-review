import { describe, expect, it } from "bun:test";
import { visibleFromCollapsedItem } from "./layoutPanelVisibility";

describe("visibleFromCollapsedItem", () => {
  it("treats a missing key as visible", () => {
    expect(visibleFromCollapsedItem(null)).toBe(true);
  });

  it("treats collapsed=true as hidden", () => {
    expect(visibleFromCollapsedItem("true")).toBe(false);
  });

  it("treats any other stored value as visible", () => {
    expect(visibleFromCollapsedItem("false")).toBe(true);
  });
});
