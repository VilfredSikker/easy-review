/**
 * Returns a scroll handler that coalesces rapid updates to once per animation
 * frame. Multiple calls before the frame fires all feed the same RAF callback,
 * which reads the latest value — so the eventual `onFrame(top)` call always
 * gets the most recent scroll position, never a stale intermediate.
 *
 * This reduces `scrollTopPx` state writes from "every scroll event" (many
 * per second during fast scrolling) to at most once per 16ms animation frame,
 * cutting the number of Svelte reactive re-renders accordingly.
 */
export function makeScrollThrottle(onFrame: (top: number) => void): (top: number) => void {
  let pending = false;
  let latest = 0;
  return function throttled(top: number): void {
    latest = top;
    if (!pending) {
      pending = true;
      requestAnimationFrame(() => {
        onFrame(latest);
        pending = false;
      });
    }
  };
}
