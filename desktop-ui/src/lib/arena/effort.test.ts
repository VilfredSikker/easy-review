import { describe, expect, it } from "vitest";
import {
  effortChoicesForModel,
  effortLabel,
  modelSupportsEffort,
  selectedModelDescription,
} from "./effort";

describe("effortChoicesForModel", () => {
  it("returns no choices when the model has no thinking levels", () => {
    expect(
      effortChoicesForModel({ effort_levels: [], is_selected: true }, "high"),
    ).toEqual([]);
    expect(modelSupportsEffort({ effort_levels: [] })).toBe(false);
  });

  it("lists advertised levels and marks the current one only when the model is selected", () => {
    const model = {
      effort_levels: ["low", "medium", "high"],
      is_selected: true,
    };
    expect(effortChoicesForModel(model, "high")).toEqual([
      { id: "low", label: "Low", selected: false },
      { id: "medium", label: "Medium", selected: false },
      { id: "high", label: "High", selected: true },
    ]);
    expect(
      effortChoicesForModel({ ...model, is_selected: false }, "high").map((c) => c.selected),
    ).toEqual([false, false, false]);
  });
});

describe("selectedModelDescription", () => {
  it("stays empty for an unselected model", () => {
    expect(
      selectedModelDescription({ is_selected: false, effort_levels: ["high"] }, "high"),
    ).toBe("");
  });

  it("includes the current effort when the selected model has thinking levels", () => {
    expect(
      selectedModelDescription({ is_selected: true, effort_levels: ["high"] }, "high"),
    ).toBe("currently selected · High");
  });

  it("omits effort when the selected model has none", () => {
    expect(selectedModelDescription({ is_selected: true, effort_levels: [] }, null)).toBe(
      "currently selected",
    );
  });
});

describe("effortLabel", () => {
  it("title-cases catalog ids", () => {
    expect(effortLabel("high")).toBe("High");
    expect(effortLabel("xhigh")).toBe("XHigh");
  });
});
