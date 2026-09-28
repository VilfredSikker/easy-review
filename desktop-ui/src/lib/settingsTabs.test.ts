import { describe, expect, it } from "bun:test";
import { tabForSection } from "$lib/settingsTabs";

describe("tabForSection", () => {
  it("routes the agent and command sections off General", () => {
    expect(tabForSection("Agent")).toBe("ai");
    expect(tabForSection("Commands")).toBe("review");
  });

  it("keeps known General sections on General", () => {
    expect(tabForSection("Appearance")).toBe("general");
    expect(tabForSection("Inbox notifications")).toBe("general");
  });

  it("puts an unmapped or untitled section on General rather than hiding it", () => {
    expect(tabForSection("Some Future Section")).toBe("general");
    expect(tabForSection(null)).toBe("general");
  });
});
