import { reviewedClearedBySync, shortOid } from "./shellBanners";
import type { AppSnapshot } from "./types";

/** `force_refresh_diff`'s reply: a snapshot plus the Reviewed marks the engine cleared. */
export type SyncReply = AppSnapshot & { reviewed_cleared?: number };

type Before = Pick<AppSnapshot, "files" | "diff_stale"> | null;

/**
 * The one toast a Sync leaves: where the diff landed, and how many Reviewed
 * marks went with the files that changed. The engine's count wins; comparing
 * snapshots is the fallback for a reply that lost it.
 */
export function syncToastMessage(before: Before, after: Pick<SyncReply, "files" | "reviewed_cleared">): string {
  const target = before?.diff_stale?.latest_oid ?? null;
  const landed = target ? `Synced to ${shortOid(target)}` : "Diff refreshed";
  const cleared = after.reviewed_cleared ?? reviewedClearedBySync(before?.files ?? [], after.files);
  if (cleared <= 0) return landed;
  return `${landed} · Reviewed cleared on ${cleared} changed file${cleared === 1 ? "" : "s"}`;
}
