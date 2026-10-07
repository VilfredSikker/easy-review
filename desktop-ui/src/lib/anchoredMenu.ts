/** Width of an anchored dropdown, in px (`w-64` in `AnchoredMenu.svelte`). */
export const ANCHORED_MENU_WIDTH = 256;

/** Gap kept between the dropdown and the window edge, in px. */
const EDGE_GAP = 8;

/**
 * Where a dropdown opened from `button` sits, in viewport coordinates. Menus
 * are drawn `fixed` because the right panel clips overflow: anchored inside
 * it, a menu wider than the space left of the button was cut off at the
 * panel's edge. Right-aligned under the button, then kept inside the window.
 */
export function anchoredMenuPosition(
  button: { right: number; bottom: number },
  viewportWidth: number,
): { top: number; left: number } {
  const maxLeft = viewportWidth - ANCHORED_MENU_WIDTH - EDGE_GAP;
  const left = Math.max(EDGE_GAP, Math.min(button.right - ANCHORED_MENU_WIDTH, maxLeft));
  return { top: button.bottom + 4, left };
}
