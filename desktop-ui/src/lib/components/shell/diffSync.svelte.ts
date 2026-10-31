import { app } from "$lib/stores/app.svelte";

// One flag for the Stale pill and the stale banner, so a Sync started from
// either spins both and neither can start a second fetch.
let syncing = $state(false);

export const diffSync = {
  get syncing(): boolean {
    return syncing;
  },

  /** Rebuild the diff against origin. `app.cmd` toasts where it landed and
   *  the Reviewed marks it cleared, for every Sync path alike. */
  async sync(): Promise<void> {
    if (syncing) return;
    syncing = true;
    try {
      await app.cmd("force_refresh_diff");
    } finally {
      syncing = false;
    }
  },
};
