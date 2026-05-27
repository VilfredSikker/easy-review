import { describe, expect, it } from "bun:test";
import { threadFileLineRef, threadLineRangeLabel, threadLineRefSuffix } from "./threadLabels";
import type { ThreadSnapshot } from "./types";

function thread(line: number, line_end?: number | null): Pick<ThreadSnapshot, "file" | "line" | "line_end"> {
  return { file: "src/a.ts", line, line_end };
}

describe("threadLabels", () => {
  it("single-line labels", () => {
    const t = thread(183);
    expect(threadLineRangeLabel(t)).toBe("line 183");
    expect(threadFileLineRef(t)).toBe("src/a.ts:183");
    expect(threadLineRefSuffix(t)).toBe("183");
  });

  it("multi-line labels", () => {
    const t = thread(183, 186);
    expect(threadLineRangeLabel(t)).toBe("lines 183–186");
    expect(threadFileLineRef(t)).toBe("src/a.ts:183–186");
    expect(threadLineRefSuffix(t)).toBe("183–186");
  });
});
