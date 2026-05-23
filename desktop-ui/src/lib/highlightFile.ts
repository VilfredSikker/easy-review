import { langForPath } from "./extToLang";
import { highlightLines } from "./highlightClient";
import type { HunkHighlight } from "./highlightCache";
import type { SyntaxTheme } from "./syntaxThemes";
import type { FileSnapshot, SpanSnapshot } from "./types";

function flattenHighlightLines(file: FileSnapshot): string[] {
  const lines: string[] = [];
  for (const hunk of file.hunks) {
    for (const line of hunk.lines) {
      if (line.kind === "fold") continue;
      lines.push(line.text);
    }
  }
  return lines;
}

/** Convert per-line worker spans back into per-hunk layout for `applyHunkSpans`. */
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
  const texts = flattenHighlightLines(file);
  if (texts.length === 0) return [];
  const lang = langForPath(file.path);
  const lineSpans = await highlightLines(texts, lang, theme);
  return spansToHunks(file, lineSpans);
}
