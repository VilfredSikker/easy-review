/** The categorical `--color-commit-1..6` series, cycled per commit in order of
 *  appearance, so a commit keeps one colour across the commits list, the
 *  Mark lines bars and the per-file chip. A series of its own rather than the
 *  semantic status tokens, so a commit never reads as "warning" or "success".
 *  Tokens, not hex (ADR 0030): they follow the active theme. */
export const COMMIT_COLOR_TOKENS = [
  "--color-commit-1",
  "--color-commit-2",
  "--color-commit-3",
  "--color-commit-4",
  "--color-commit-5",
  "--color-commit-6",
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
