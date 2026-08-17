/** Sentinel when a queued invoke is skipped because a newer call superseded it. */
export const LATEST_INVOKE_SKIPPED = Symbol("latest-invoke-skipped");

export type LatestInvokeResult<T> = T | typeof LATEST_INVOKE_SKIPPED;

/**
 * Serializes async work in enqueue order. Skips `fn` when `isCurrent` is false
 * at the moment the slot runs, so a later click can cancel an earlier IPC.
 */
export function createLatestInvokeQueue() {
  let tail: Promise<unknown> = Promise.resolve();

  return {
    enqueue<T>(
      isCurrent: () => boolean,
      fn: () => Promise<T>,
    ): Promise<LatestInvokeResult<T>> {
      const run = async (): Promise<LatestInvokeResult<T>> => {
        if (!isCurrent()) return LATEST_INVOKE_SKIPPED;
        return fn();
      };
      const p = tail.then(run, run);
      tail = p.then(
        () => undefined,
        () => undefined,
      );
      return p;
    },
  };
}
