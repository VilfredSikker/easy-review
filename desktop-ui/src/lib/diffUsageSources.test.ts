import { expect, test } from "bun:test";
import { collectDiffUsageSources } from "./diffUsageSources";
import type { CrossFileModel } from "./diffRenderModel";
import { splitRows } from "./splitRows";
import { fileMediaCombobox } from "./stories/fixtures";
import type { FileSnapshot, LineSnapshot } from "./types";

test("split search retains rows after highlighting replaces source line objects", () => {
  const before: LineSnapshot[] = [
    { kind: "del", old_num: 1, new_num: null, text: "old document" },
    { kind: "add", old_num: null, new_num: 1, text: "new document" },
  ];
  const file: FileSnapshot = { ...fileMediaCombobox, path: "README.md", hunks: [{ header: "@@ -1 +1 @@", old_start: 1, new_start: 1, old_count: 1, new_count: 1, threads: [], lines: before.map((line) => ({ ...line, spans: [{ text: line.text, color: "blue" }] })) }] };
  const cached = splitRows(before);
  const model: Pick<CrossFileModel, "rows" | "splitRowsByFile"> = {
    rows: cached.map((_, i) => ({ type: "content-split", filePath: file.path, hunkIdx: 0, splitRowIdx: i, identity: `split-${i}`, height: 20 })),
    splitRowsByFile: new Map([[file.path, [cached]]]),
  };
  expect(file.hunks[0].lines[0]).not.toBe(before[0]);
  const sources = collectDiffUsageSources([file], model);
  expect(sources.map((source) => source.rowIdx)).toEqual([0, 0]);
  expect(sources.map((source) => source.text)).toEqual(["old document", "new document"]);
  expect(collectDiffUsageSources([file], { rows: [], splitRowsByFile: new Map() }).map((source) => source.rowIdx)).toEqual([-1, -1]);
});
