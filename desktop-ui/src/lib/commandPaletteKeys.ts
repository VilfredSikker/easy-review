/** True when the command palette search field is the keydown target. */
export function isPaletteSearchFocused(target: EventTarget | null): boolean {
  if (!target || typeof target !== "object") return false;
  const dataset = (target as { dataset?: { paletteSearch?: string } }).dataset;
  return dataset?.paletteSearch === "true";
}

/**
 * Letter shortcuts and `/` (focus search) run only when the search field is
 * not focused. While it is focused, the same keys type into the filter.
 */
export function paletteQuickActionKey(
  e: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean },
  searchFocused: boolean,
): "search" | "letter" | null {
  if (searchFocused) return null;
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  if (e.key === "/") return "search";
  if (e.key.length === 1 && /^[a-zA-Z]$/.test(e.key)) return "letter";
  return null;
}
