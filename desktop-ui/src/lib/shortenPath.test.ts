import { describe, expect, it } from "bun:test";
import { charsForMonoWidth, shortenPath, splitPathForDisplay } from "./shortenPath";

describe("shortenPath", () => {
  it("returns path unchanged when it fits", () => {
    expect(shortenPath("src/main.rs", 30)).toBe("src/main.rs");
  });

  it("truncates directory prefix when filename fits", () => {
    expect(shortenPath("src/very/long/nested/path/main.rs", 20)).toBe("src/very/…/main.rs");
  });

  it("returns filename only when no room for directory", () => {
    expect(shortenPath("src/very/long/nested/path/main.rs", 10)).toBe("main.rs");
  });

  it("truncates long filename", () => {
    expect(shortenPath("very_long_filename_here.rs", 10)).toBe("very_long…");
  });

  it("handles maxChars zero", () => {
    expect(shortenPath("src/main.rs", 0)).toBe("…");
  });
});

describe("splitPathForDisplay", () => {
  it("splits shortened paths", () => {
    expect(splitPathForDisplay("src/very/…/main.rs")).toEqual({
      dir: "src/very/…/",
      name: "main.rs",
    });
  });
});

describe("charsForMonoWidth", () => {
  it("floors pixel width to char estimate", () => {
    expect(charsForMonoWidth(70)).toBe(10);
  });
});
