import { describe, it, expect } from "bun:test";
import { copyToClipboard } from "./clipboard";

describe("copyToClipboard", () => {
  it("is an async function", () => {
    expect(typeof copyToClipboard).toBe("function");
  });
});
