import type { CrossFileModel } from "./diffRenderModel";
import type { UsageSource } from "./referenceUsages";
import type { FileSnapshot, LineSnapshot } from "./types";

function sourceKey(path: string, hunkIdx: number, line: LineSnapshot): string {
  return JSON.stringify([path, hunkIdx, line.kind, line.old_num, line.new_num]);
}

/** Syntax highlighting can replace line objects while split rows retain their source anchors. */
export function collectDiffUsageSources(
  files: FileSnapshot[],
  model: Pick<CrossFileModel, "rows" | "splitRowsByFile">,
): UsageSource[] {
  const lineToRow = new Map<string, number>();
  const byPath = new Map(files.map((f) => [f.path, f]));
  for (let i = 0; i < model.rows.length; i++) {
    const row = model.rows[i];
    if (row.type === "content-unified") {
      const line = byPath.get(row.filePath)?.hunks[row.hunkIdx]?.lines[row.lineIdx];
      if (line) lineToRow.set(sourceKey(row.filePath, row.hunkIdx, line), i);
    } else if (row.type === "content-split") {
      const split = model.splitRowsByFile.get(row.filePath)?.[row.hunkIdx]?.[row.splitRowIdx];
      if (split?.left) lineToRow.set(sourceKey(row.filePath, row.hunkIdx, split.left), i);
      if (split?.right) lineToRow.set(sourceKey(row.filePath, row.hunkIdx, split.right), i);
    }
  }

  const sources: UsageSource[] = [];
  for (const file of files) {
    for (let h = 0; h < file.hunks.length; h++) {
      for (let l = 0; l < file.hunks[h].lines.length; l++) {
        const line = file.hunks[h].lines[l];
        if (line.kind === "fold") continue;
        sources.push({
          rowIdx: lineToRow.get(sourceKey(file.path, h, line)) ?? -1,
          filePath: file.path,
          lineNum: line.new_num ?? line.old_num,
          text: line.text,
          hunkIdx: h,
          lineIdx: l,
        });
      }
    }
  }
  return sources;
}
