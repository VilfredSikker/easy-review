import { describe, expect, it } from "bun:test";
import { fileStatusDisplay, riskDotClass } from "./fileStatus";

describe("fileStatusDisplay", () => {
  it("maps git statuses to glyphs, icons, and titles", () => {
    expect(fileStatusDisplay("added")).toMatchObject({
      glyph: "+",
      icon: "plus-circle",
      title: "New file",
    });
    expect(fileStatusDisplay("deleted")).toMatchObject({
      glyph: "−",
      icon: "minus-circle",
      title: "Deleted",
    });
    expect(fileStatusDisplay("modified")).toMatchObject({
      glyph: "~",
      icon: "pencil",
      title: "Modified",
    });
    expect(fileStatusDisplay("renamed")).toMatchObject({
      glyph: "R",
      icon: "pencil",
      title: "Renamed",
    });
    expect(fileStatusDisplay("copied")).toMatchObject({
      glyph: "C",
      icon: "plus-circle",
      title: "Copied",
    });
    expect(fileStatusDisplay("unmerged")).toMatchObject({
      glyph: "!",
      icon: "alert",
      title: "Unmerged",
    });
  });
});

describe("riskDotClass", () => {
  it("draws every verdict in its own colour", () => {
    // The tree and the risk queue both call this, so a level that fell through
    // to another level's colour would show the same file two ways at once.
    expect(riskDotClass("high")).toBe("bg-risk-high");
    expect(riskDotClass("med")).toBe("bg-risk-med");
    expect(riskDotClass("low")).toBe("bg-risk-low");
  });
});
