/** Collapsed localStorage values are inverted: `"true"` means hidden. */
export function visibleFromCollapsedItem(item: string | null): boolean {
  return item !== "true";
}
