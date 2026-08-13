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
