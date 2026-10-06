import { describe, expect, it } from "bun:test";
import { ANCHORED_MENU_WIDTH, anchoredMenuPosition } from "./anchoredMenu";

describe("anchoredMenuPosition", () => {
  it("right-aligns under the button when there is room", () => {
    expect(anchoredMenuPosition({ right: 900, bottom: 100 }, 1200)).toEqual({
      top: 104,
      left: 900 - ANCHORED_MENU_WIDTH,
    });
  });

  it("stays inside the window when the button is near the left edge", () => {
    // A narrow panel near the window's left edge: right-aligning would start
    // the menu off-screen.
    expect(anchoredMenuPosition({ right: 120, bottom: 100 }, 1200).left).toBe(8);
  });

  it("stays inside the window on the right", () => {
    const { left } = anchoredMenuPosition({ right: 1300, bottom: 100 }, 1200);
    expect(left + ANCHORED_MENU_WIDTH).toBe(1192);
  });
});
