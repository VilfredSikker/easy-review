import { langForPath } from "./extToLang";
import { highlightLines } from "./highlightClient";
import type { HunkHighlight } from "./highlightCache";
import {
  buildHighlightSides,
  fileHasDeletions,
  fileNeedsSyntaxSpans,
  spansToHunksFromSides,
} from "./highlightPlan";
import type { SyntaxTheme } from "./syntaxThemes";
import type { FileSnapshot, SpanSnapshot } from "./types";

export { fileNeedsSyntaxSpans, lineNeedsSyntaxSpans } from "./highlightPlan";

/** Convert flat worker spans (legacy single-buffer layout) into per-hunk layout. */
export function spansToHunks(file: FileSnapshot, lineSpans: SpanSnapshot[][]): HunkHighlight[] {
  let cursor = 0;
  return file.hunks.map((hunk, hunk_index) => ({
    hunk_index,
    lines: hunk.lines.map((line) => {
      if (line.kind === "fold") return [] as SpanSnapshot[];
      const spans = lineSpans[cursor] ?? [];
      cursor += 1;
      return spans;
    }),
  }));
}

export async function highlightFile(
  file: FileSnapshot,
  theme: SyntaxTheme,
): Promise<HunkHighlight[]> {
  const { newSide, oldSide } = buildHighlightSides(file);
  if (newSide.texts.length === 0 && oldSide.texts.length === 0) return [];

  const lang = langForPath(file.path);
  const needOld = fileHasDeletions(file) && oldSide.texts.length > 0;

  const [newSpans, oldSpans] = await Promise.all([
    newSide.texts.length > 0
      ? highlightLines(newSide.texts, lang, theme)
      : Promise.resolve([] as SpanSnapshot[][]),
    needOld ? highlightLines(oldSide.texts, lang, theme) : Promise.resolve([] as SpanSnapshot[][]),
  ]);

  return spansToHunksFromSides(file, newSide, newSpans, oldSide, oldSpans);
}
