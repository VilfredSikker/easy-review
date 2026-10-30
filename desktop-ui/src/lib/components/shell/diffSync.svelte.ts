import { app } from "$lib/stores/app.svelte";
import { reviewedClearedBySync, shortOid } from "$lib/shellBanners";

// One flag for the Stale pill and the stale banner, so a Sync started from
// either spins both and neither can start a second fetch.
let syncing = $state(false);

export const diffSync = {
  get syncing(): boolean {
    return syncing;
  },

  /** Rebuild the diff against origin, then say how many Reviewed marks went. */
  async sync(): Promise<void> {
    if (syncing) return;
    const before = app.snapshot?.files ?? [];
    const target = app.snapshot?.diff_stale?.latest_oid ?? null;
    syncing = true;
    try {
      await app.cmd("force_refresh_diff", undefined, { throwOnError: true });
    } catch {
      return; // app.cmd already reported the failure
    } finally {
      syncing = false;
    }
    // The engine counts the marks it cleared (`reviewed_cleared`); fall back
    // to comparing snapshots when the field did not survive the merge.
    const after = app.snapshot as ({ reviewed_cleared?: number } & typeof app.snapshot) | null;
    const cleared = after?.reviewed_cleared ?? reviewedClearedBySync(before, after?.files ?? []);
    if (cleared > 0) {
      const to = target ? `Synced to ${shortOid(target)} · ` : "";
      app.showToast("info", `${to}Reviewed cleared on ${cleared} changed file${cleared === 1 ? "" : "s"}`);
    }
  },
};
