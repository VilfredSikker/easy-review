import type { ArenaRunSnapshot } from "$lib/types/arena";
import { isSeededRunFromSnapshot } from "./runKind";

export type RunActionResult<T> = { kind: "performed"; value: T } | { kind: "readOnlyVerification" };

/** Run a result mutation only for ordinary manual runs, never seeded verification. */
export async function performUnlessSeeded<T>(
  snap: ArenaRunSnapshot,
  action: () => Promise<T>,
): Promise<RunActionResult<T>> {
  if (isSeededRunFromSnapshot(snap)) return { kind: "readOnlyVerification" };
  return { kind: "performed", value: await action() };
}
