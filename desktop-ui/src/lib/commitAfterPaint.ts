/**
 * First measured wrap width applies immediately (no unwrapped flash).
 * Later changes wait until after paint so panel collapse can hit the
 * previous render-model cache identity in the same frame as the layout.
 */
export function shouldCommitWrapColsImmediately(
  next: number | null,
  committed: number | null,
): boolean {
  return committed === null && next !== null;
}

/** Double rAF: layout + ResizeObserver settle, then one frame of paint. */
export function scheduleAfterPaint(cb: () => void): () => void {
  let inner = 0;
  const outer = requestAnimationFrame(() => {
    inner = requestAnimationFrame(cb);
  });
  return () => {
    cancelAnimationFrame(outer);
    if (inner) cancelAnimationFrame(inner);
  };
}
