import { describe, expect, it } from "bun:test";
import { OTHER_FILES_TITLE, filesByPillar } from "./pillarFiles";
import type { FileSnapshot, PillarSnapshot } from "./types";

const file = (path: string) => ({ path }) as FileSnapshot;
const pillar = (id: string, paths: string[]) =>
  ({
    id,
    title: `Pillar ${id}`,
    files: paths.map((path) => ({ path, reason: "", findingIds: [] })),
  }) as unknown as PillarSnapshot;

describe("filesByPillar", () => {
  it("groups files under their pillar in tour order", () => {
    const g = filesByPillar(
      [pillar("1", ["b", "a"]), pillar("2", ["c"])],
      [file("a"), file("b"), file("c")],
    );
    expect(g.map((x) => x.files.map((f) => f.path))).toEqual([["b", "a"], ["c"]]);
    expect(g.map((x) => x.pillarId)).toEqual(["1", "2"]);
  });

  it("collects uncovered files under Other files", () => {
    const g = filesByPillar([pillar("1", ["a"])], [file("a"), file("x"), file("y")]);
    expect(g).toHaveLength(2);
    expect(g[1]).toMatchObject({ pillarId: null, title: OTHER_FILES_TITLE });
    expect(g[1].files.map((f) => f.path)).toEqual(["x", "y"]);
  });

  it("omits Other files when the tour covers everything", () => {
    expect(filesByPillar([pillar("1", ["a"])], [file("a")])).toHaveLength(1);
  });

  it("ignores tour paths missing from the diff and lists a shared file once", () => {
    const g = filesByPillar(
      [pillar("1", ["a", "gone"]), pillar("2", ["a"])],
      [file("a")],
    );
    expect(g[0].files).toHaveLength(1);
    expect(g[1].files).toHaveLength(0);
  });

  it("puts every file in Other files when there is no tour", () => {
    const g = filesByPillar([], [file("a")]);
    expect(g).toEqual([{ pillarId: null, title: OTHER_FILES_TITLE, files: [file("a")] }]);
  });
});
