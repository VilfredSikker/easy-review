/**
 * Partition a unified-diff hunk's lines into aligned left/right rows
 * for side-by-side ("split") rendering.
 *
 * Rules:
 * - A `context` line appears identically on both sides.
 * - A standalone `add` puts the line on the right with an empty left placeholder.
 * - A standalone `del` puts the line on the left with an empty right placeholder.
 * - A run of consecutive `del` then `add` lines pairs up by index (classic
 *   modify alignment). Extras on either side fall through as unpaired rows.
 * - `fold` lines pass through on both sides.
 */
import type { LineSnapshot } from "./types";

export interface SplitRow {
  left: LineSnapshot | null;
  right: LineSnapshot | null;
}

export function splitRows(lines: LineSnapshot[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.kind === "context" || line.kind === "fold") {
      rows.push({ left: line, right: line });
      i++;
      continue;
    }

    // Collect a run of consecutive del lines, then a run of consecutive add lines.
    const dels: LineSnapshot[] = [];
    while (i < lines.length && lines[i].kind === "del") {
      dels.push(lines[i]);
      i++;
    }
    const adds: LineSnapshot[] = [];
    while (i < lines.length && lines[i].kind === "add") {
      adds.push(lines[i]);
      i++;
    }

    const maxLen = Math.max(dels.length, adds.length);
    for (let k = 0; k < maxLen; k++) {
      rows.push({
        left: dels[k] ?? null,
        right: adds[k] ?? null,
      });
    }
  }
  return rows;
}
