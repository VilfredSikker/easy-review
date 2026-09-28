import { describe, expect, test } from "bun:test";
import { extChip, hiddenFindingCounts, toggledPath, toggledPaths } from "./fileTreeModel";
import type { AiSnapshot, Confidence, FlatFinding } from "./types";

describe("extChip", () => {
  test.each([
    ["ts", "TS"], ["tsx", "TSX"], ["js", "JS"], ["jsx", "JSX"], ["svelte", "SV"],
    ["css", "CSS"], ["scss", "SCS"], ["rs", "RS"], ["md", "MD"], ["json", "JSON"],
    ["toml", "TOML"], ["yaml", "YML"], ["yml", "YML"], ["html", "HTML"], ["py", "PY"],
    ["go", "GO"], ["sh", "SH"], ["bash", "SH"],
  ])("%s → %s", (ext, label) => {
    expect(extChip(ext).label).toBe(label);
  });

  test("unknown extensions use the first three letters, muted", () => {
    expect(extChip("kotlin")).toEqual({ label: "KOT", color: "var(--color-muted)" });
  });

  test("no extension renders a dot", () => {
    expect(extChip("")).toEqual({ label: "·", color: "var(--color-muted)" });
  });

  test("an inherited object key is not a known extension", () => {
    expect(extChip("constructor").label).toBe("CON");
  });
});

describe("selection toggles", () => {
  test("toggledPath flips one path without mutating the input", () => {
    const cur = new Set(["a"]);
    expect([...toggledPath(cur, "a")]).toEqual([]);
    expect([...toggledPath(cur, "b")]).toEqual(["a", "b"]);
    expect([...cur]).toEqual(["a"]);
    expect([...toggledPath(undefined, "a")]).toEqual(["a"]);
  });

  test("toggledPaths removes all when all are selected, else adds all", () => {
    const cur = new Set(["x", "a"]);
    expect([...toggledPaths(cur, ["a", "b"], true)]).toEqual(["x"]);
    expect([...toggledPaths(cur, ["a", "b"], false)]).toEqual(["x", "a", "b"]);
    expect([...cur]).toEqual(["x", "a"]);
  });
});

describe("hiddenFindingCounts", () => {
  const f = (file: string, confidence: Confidence) => ({ file, confidence }) as FlatFinding;

  test("counts findings below the gate per file", () => {
    const ai = {
      findings: [f("a", "confirmed"), f("a", "tentative"), f("a", "informational"), f("b", "dropped")],
      min_trust_default: "confirmed",
    } as Pick<AiSnapshot, "findings" | "min_trust_default">;
    const counts = hiddenFindingCounts(ai, (fallback) => fallback);
    expect([...counts]).toEqual([["a", 2], ["b", 1]]);
  });

  test("no AI snapshot hides nothing and never asks for the gate", () => {
    let asked = false;
    const counts = hiddenFindingCounts(null, (fallback) => { asked = true; return fallback; });
    expect(counts.size).toBe(0);
    expect(asked).toBe(false);
  });
});
