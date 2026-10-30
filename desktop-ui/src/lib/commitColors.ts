/** Semantic accent tokens cycled per commit, in order of appearance, so a
 *  commit keeps one colour across the strip, file rows and blame gutter.
 *  Tokens, not hex (ADR 0030): they follow the active theme. */
export const COMMIT_COLOR_TOKENS = [
  "--color-periwinkle",
  "--color-info",
  "--color-warning",
  "--color-success",
  "--color-emphasis",
  "--color-action",
] as const;

export function commitColorToken(index: number): string {
  const n = COMMIT_COLOR_TOKENS.length;
  const i = Number.isFinite(index) ? Math.trunc(index) : 0;
  return COMMIT_COLOR_TOKENS[((i % n) + n) % n];
}

export function commitColorVar(index: number): string {
  return `var(${commitColorToken(index)})`;
}

/** Index of the first commit touching `path`; -1 when none does. */
export function commitIndexForFile(
  commits: ReadonlyArray<{ files: readonly string[] }>,
  path: string,
): number {
  return commits.findIndex((c) => c.files.includes(path));
}
