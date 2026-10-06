import { describe, expect, it } from "bun:test";
import { countChecks } from "./checkCounts";

describe("countChecks", () => {
  it("counts both spellings of pass and fail, and pending by status", () => {
    expect(
      countChecks([
        { name: "a", status: "COMPLETED", conclusion: "SUCCESS", url: null },
        { name: "b", status: "COMPLETED", conclusion: "pass", url: null },
        { name: "c", status: "COMPLETED", conclusion: "FAILURE", url: null },
        { name: "d", status: "COMPLETED", conclusion: "fail", url: null },
        { name: "e", status: "PENDING", conclusion: "", url: null },
        { name: "f", status: "COMPLETED", conclusion: "SKIPPED", url: null },
      ]),
    ).toEqual({ pass: 2, fail: 2, pending: 1, total: 6 });
  });

  it("is all zeros for no checks", () => {
    expect(countChecks([])).toEqual({ pass: 0, fail: 0, pending: 0, total: 0 });
  });
});
