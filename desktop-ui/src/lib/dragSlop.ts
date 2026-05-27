/** Shared drag slop math for diff line selection (unit-testable). */
export const DRAG_SLOP_PX = 4;

export function exceededDragSlop(
  anchorX: number,
  anchorY: number,
  clientX: number,
  clientY: number,
  slopPx = DRAG_SLOP_PX,
): boolean {
  const dx = clientX - anchorX;
  const dy = clientY - anchorY;
  return dx * dx + dy * dy > slopPx * slopPx;
}
