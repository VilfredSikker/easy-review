import { describe, expect, it } from "bun:test";
import {
  composerSubmission,
  hoverTarget,
  iframeClick,
  objectField,
  reanchorUpdates,
  stringField,
} from "./browserPayload";

describe("browserPayload field readers", () => {
  it("keeps strings and non-null objects, drops everything else", () => {
    expect(stringField({ a: "x", b: 1 }, "a")).toBe("x");
    expect(stringField({ a: "x", b: 1 }, "b")).toBeNull();
    expect(objectField({ o: { k: 1 } }, "o")).toEqual({ k: 1 });
    expect(objectField({ o: null }, "o")).toBeNull();
    expect(objectField({ o: "str" }, "o")).toBeNull();
    expect(objectField({}, "o")).toBeNull();
  });
});

describe("composerSubmission", () => {
  it("reads the box, falling back to 24x24 sizes and zero offsets", () => {
    expect(
      composerSubmission({
        box: [10, "20", 0, 40],
        text: "pin",
        selector: "#a",
        element_context: "button",
        dom_context: { tag: "button" },
      }),
    ).toEqual({
      bbox: [10, 20, 24, 40],
      selector: "#a",
      text: "pin",
      elementContext: "button",
      domContext: { tag: "button" },
    });
  });

  it("defaults a missing or short box and non-string fields", () => {
    expect(composerSubmission({ box: [1, 2], text: 5 })).toEqual({
      bbox: [0, 0, 24, 24],
      selector: null,
      text: "",
      elementContext: null,
      domContext: null,
    });
  });
});

describe("hoverTarget", () => {
  it("is null without a rect", () => {
    expect(hoverTarget({ selector: "#a" })).toBeNull();
    expect(hoverTarget({ rect: null })).toBeNull();
  });

  it("carries the rect and optional context", () => {
    const rect = { left: 1, top: 2, width: 3, height: 4 };
    expect(hoverTarget({ rect, selector: "#a", element_context: 7 })).toEqual({
      selector: "#a",
      rect,
      element_context: null,
      dom_context: null,
    });
  });
});

describe("reanchorUpdates", () => {
  it("normalises fresh and new_box, and tolerates a missing list", () => {
    expect(
      reanchorUpdates({
        results: [
          { id: "a", fresh: 1, new_box: [1, 2, 3, 4] },
          { id: "b", fresh: false },
        ],
      }),
    ).toEqual([
      { id: "a", fresh: true, new_box: [1, 2, 3, 4] },
      { id: "b", fresh: false, new_box: null },
    ]);
    expect(reanchorUpdates({ results: "nope" })).toEqual([]);
  });
});

describe("iframeClick", () => {
  it("coerces coordinates and drops non-string context", () => {
    expect(iframeClick({ x: "5", y: undefined, w: 10, h: "bad", selector: 3 })).toEqual({
      x: 5,
      y: 0,
      w: 10,
      h: 0,
      selector: null,
      element_context: null,
      dom_context: null,
    });
  });
});
