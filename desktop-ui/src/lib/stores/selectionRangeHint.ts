/**
 * Composer hint after the range label. Pure so it is testable: the rune store
 * that calls it cannot load under `bun test`.
 */
export function selectionRangeHint(start: number | null, end: number | null): string {
  if (start === null || end === null) return "";
  const count = Math.abs(end - start) + 1;
  return count === 1 ? "shift-click another line for a range" : `${count} lines`;
}
