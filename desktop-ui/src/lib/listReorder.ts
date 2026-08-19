/**
 * Insertion-slot math for HTML5 list reorder (tabs, sidebar projects).
 *
 * `dropSlot` is the gap index in `0..=len` (before item 0 … after the last
 * item). Removing `from` before inserting means a slot past the source maps
 * to `slot - 1` as the destination index.
 */

/** Gap to insert at, from pointer position along the list axis. */
export function dropSlot(
  clientPos: number,
  start: number,
  size: number,
  idx: number,
): number {
  const after = clientPos > start + size / 2;
  return after ? idx + 1 : idx;
}

/** Destination index after removing `from`, given a drop `slot` (`0..=len`). */
export function destIndexAfterRemove(from: number, slot: number): number {
  return slot > from ? slot - 1 : slot;
}

/** Copy of `items` with the entry at `from` moved to `dest`. */
export function movedIds<T>(items: readonly T[], from: number, dest: number): T[] {
  const next = items.slice();
  if (from === dest || from < 0 || dest < 0 || from >= next.length || dest >= next.length) {
    return next;
  }
  const [item] = next.splice(from, 1);
  next.splice(dest, 0, item);
  return next;
}
