import { describe, expect, test } from "bun:test";
import { splitToast } from "./toastModel";

describe("splitToast", () => {
  test("splits success on the first separator", () => {
    expect(splitToast("success", "Synced to 9f3e1a2 · Reviewed cleared on 2 files")).toEqual({
      title: "Synced to 9f3e1a2",
      detail: "Reviewed cleared on 2 files",
    });
  });
  test("keeps later separators in the detail", () => {
    expect(splitToast("info", "a · b · c")).toEqual({ title: "a", detail: "b · c" });
  });
  test("no separator means no detail", () => {
    expect(splitToast("info", "Copied")).toEqual({ title: "Copied", detail: "" });
  });
  test("errors and warnings stay whole", () => {
    expect(splitToast("error", "cmd: x · y")).toEqual({ title: "cmd: x · y", detail: "" });
    expect(splitToast("warn", "a · b").detail).toBe("");
  });
});
