import { describe, expect, test } from "bun:test";
import { terminalRepoName } from "./terminalTabs";

describe("terminalRepoName", () => {
  test("takes the last path segment", () => {
    expect(terminalRepoName("/home/u/code/discovery")).toBe("discovery");
    expect(terminalRepoName("C:\\code\\discovery\\")).toBe("discovery");
  });
  test("falls back for empty cwd", () => {
    expect(terminalRepoName("")).toBe("repo");
  });
});
