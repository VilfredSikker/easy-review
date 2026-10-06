import type { DocumentBlock } from "./documentPreview";
import type { FileSnapshot, ThreadSnapshot } from "./types";

/** Inclusive 1-based source line range. */
export interface LineRange {
  start: number;
  end: number;
}

// Rendered text drops markdown syntax, so compare with it stripped from the source.
const MARKUP = /[*_~`#>|[\]()!\\]/g;

function normalize(text: string): string {
  // A link renders as its label, so drop the target.
  return text.replace(/\]\([^)]*\)/g, " ").replace(MARKUP, " ").replace(/\s+/g, " ").trim().toLowerCase();
}

function words(text: string): string[] {
  return normalize(text).split(" ").filter(Boolean);
}

/**
 * Narrow a highlight in the rendered document to the source lines holding it.
 * The highlight is located by its first and last few words inside `range`, the
 * lines of the blocks it touched; when either end cannot be found the whole
 * range is returned, which still points at the right blocks.
 */
export function quoteSourceLines(source: string, range: LineRange, quote: string): LineRange {
  const quoteWords = words(quote);
  if (quoteWords.length === 0) return range;
  const lines = source.split("\n");
  // One normalized string over the range, with the line each character came from.
  let haystack = "";
  const lineAt: number[] = [];
  for (let line = range.start; line <= Math.min(range.end, lines.length); line++) {
    const text = normalize(lines[line - 1]);
    if (!text) continue;
    if (haystack) {
      haystack += " ";
      lineAt.push(line);
    }
    haystack += text;
    // One entry per UTF-16 unit, the unit `indexOf` counts in.
    lineAt.push(...new Array<number>(text.length).fill(line));
  }
  const span = Math.min(3, quoteWords.length);
  const head = quoteWords.slice(0, span).join(" ");
  const tail = quoteWords.slice(-span).join(" ");
  const headAt = findPhrase(haystack, head, 0);
  if (headAt === -1) return range;
  const tailAt = findPhrase(haystack, tail, headAt);
  if (tailAt === -1) return range;
  return { start: lineAt[headAt], end: lineAt[tailAt + tail.length - 1] };
}

/**
 * Where `phrase` starts in `haystack` at or after `from`, preferring a match on
 * whole words so a short highlight like "is" does not land inside "this". A
 * highlight that starts or ends mid-word only matches unbounded, so that is the
 * fallback.
 */
function findPhrase(haystack: string, phrase: string, from: number): number {
  const padded = ` ${haystack} `;
  const bounded = padded.indexOf(` ${phrase} `, from);
  return bounded !== -1 ? bounded : haystack.indexOf(phrase, from);
}

/**
 * The hunk holding every line of `range` on the new side, or null when any
 * line sits outside the diff. Only an in-diff range can carry a GitHub
 * comment, and only it shows in the diff view too.
 */
export function hunkForLines(file: FileSnapshot, range: LineRange): number | null {
  for (let hunkIdx = 0; hunkIdx < file.hunks.length; hunkIdx++) {
    const shown = new Set<number>();
    for (const line of file.hunks[hunkIdx].lines) {
      if (line.kind !== "del" && line.kind !== "fold" && line.new_num !== null) shown.add(line.new_num);
    }
    let all = true;
    for (let n = range.start; n <= range.end && all; n++) all = shown.has(n);
    if (all) return hunkIdx;
  }
  return null;
}

/** Fold a highlight into the quote line that opens a saved question or note. */
export function quotedBody(quote: string, body: string): string {
  const flat = quote.replace(/\s+/g, " ").trim();
  const clipped = flat.length > 200 ? `${flat.slice(0, 199)}…` : flat;
  return `> ${clipped}\n\n${body.trim()}`;
}

/**
 * Place each new-side thread on the file after the block holding its line.
 * File-level threads (line 0) are left out. Threads whose line no block holds
 * (a blank line, or past a shortened file) go under the key `-1`, shown after
 * the document.
 */
export function threadsByBlock(
  blocks: readonly DocumentBlock[],
  threads: readonly ThreadSnapshot[],
  path: string,
): Map<number, ThreadSnapshot[]> {
  const placed = new Map<number, ThreadSnapshot[]>();
  for (const thread of threads) {
    if (thread.file !== path || thread.side === "LEFT" || thread.resolved || thread.line < 1) continue;
    const block = blocks.find((b) => b.startLine <= thread.line && thread.line <= b.endLine);
    const key = block?.startLine ?? -1;
    placed.set(key, [...(placed.get(key) ?? []), thread]);
  }
  return placed;
}
