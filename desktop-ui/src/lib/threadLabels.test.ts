import { describe, expect, it } from "bun:test";
import { threadLineRefSuffix } from "./threadLabels";
import type { ThreadSnapshot } from "./types";

function thread(line: number, line_end?: number | null): Pick<ThreadSnapshot, "file" | "line" | "line_end"> {
  return { file: "src/a.ts", line, line_end };
}

describe("threadLabels", () => {
  it("single-line suffix", () => {
    expect(threadLineRefSuffix(thread(183))).toBe("183");
  });

  it("multi-line suffix", () => {
    expect(threadLineRefSuffix(thread(183, 186))).toBe("183–186");
  });
});
