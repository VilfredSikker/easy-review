import type { FlowAcceptanceOrigin } from "$lib/arenaFlowAcceptance";

export interface OverlayIdentity {
  runId: string | null;
  flowOrigin: FlowAcceptanceOrigin | null;
}

function sameIdentity(left: OverlayIdentity, right: OverlayIdentity): boolean {
  return left.runId === right.runId &&
    left.flowOrigin?.runId === right.flowOrigin?.runId &&
    left.flowOrigin?.erDir === right.flowOrigin?.erDir;
}

/** Commit an async refresh only if the overlay still owns the same run and
 * flow bucket after the read finishes. */
export async function refreshOverlayIfCurrent<T>(
  requested: OverlayIdentity,
  current: () => OverlayIdentity,
  load: () => Promise<T>,
  commit: (value: T) => void,
): Promise<boolean> {
  const value = await load();
  if (!sameIdentity(requested, current())) return false;
  commit(value);
  return true;
}
