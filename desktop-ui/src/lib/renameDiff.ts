/** Where a rename's old and new names actually differ, so the header can mark
 *  only that span instead of two near-identical long names. */
export interface RenameSpans {
  /** [start, end) of the changed span in the old name. */
  old: [number, number];
  /** [start, end) of the changed span in the new name. */
  new: [number, number];
}

export function renameSpans(was: string, now: string): RenameSpans {
  let pre = 0;
  const max = Math.min(was.length, now.length);
  while (pre < max && was[pre] === now[pre]) pre++;
  let suf = 0;
  while (suf < max - pre && was[was.length - 1 - suf] === now[now.length - 1 - suf]) suf++;
  return { old: [pre, was.length - suf], new: [pre, now.length - suf] };
}

export function splitSpan(text: string, span: [number, number]): [string, string, string] {
  return [text.slice(0, span[0]), text.slice(span[0], span[1]), text.slice(span[1])];
}

/** Basename of the old path when it sits in the same directory as the new one;
 *  a move across directories keeps the full old path so the change is visible. */
export function renamedFromLabel(oldPath: string, path: string): string {
  const dirOf = (p: string) => p.slice(0, p.lastIndexOf("/") + 1);
  return dirOf(oldPath) === dirOf(path) ? oldPath.slice(dirOf(oldPath).length) : oldPath;
}

export function similarityLabel(similarity: number | null | undefined): string {
  return similarity == null ? "" : `similarity ${Math.round(similarity)}%`;
}
