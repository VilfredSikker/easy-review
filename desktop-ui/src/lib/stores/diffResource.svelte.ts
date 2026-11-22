import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { FileSnapshot } from "$lib/types";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { DiffAdapter, diffKey, prListRow, type PrHead } from "./diffAdapter";
import { hunksResource } from "./hunksResource.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

function activePrHead(): PrHead | null {
  const tab = tabsResource.activeTab;
  const row = prListRow(resources, tab?.remote ?? null, tab?.pr_number ?? null);
  return row ? { head_oid: row.headOid, head_ref: row.headRef, base_ref: row.baseRef } : null;
}

/** The active tab's file list, mode, base and staleness, from the snapshot or `diff(tab)` by build. */
export const diffResource = new DiffAdapter({
  flagOn,
  store: resources,
  snapshot: () => app.appSnapshot,
  activeKey: () => tabsResource.activeKey,
  keyAt: (idx) => tabsResource.keyAt(idx),
  prHead: activePrHead,
  // The threads come from the App's snapshot until `annotations(tab)` serves them.
  hunks: { get: (hash) => hunksResource.get(hash), threads: () => app.appSnapshot?.ai?.threads ?? [] },
});

/**
 * Fetches the active tab's hunks for `files` by the hashes their rows name,
 * when the page draws them that way; false when the App's snapshot carries them.
 */
export async function loadHashedHunks(files: readonly FileSnapshot[]): Promise<boolean> {
  const tab = tabsResource.activeKey;
  if (!diffResource.hashed() || tab === null) return false;
  await hunksResource.load(tab, files.map((f) => ({ path: f.path, hash: f.cache_key })));
  return true;
}

/** Shows `key` while the effect that called this runs. */
function showDiff(key: TabKey | null): (() => void) | undefined {
  return key === null ? undefined : resources.show(diffKey(key));
}

// With the flag on the page reads the diff fields from `diff(tab)`, the diff
// commands go through er_send, the active tab's diff is shown, and so is the
// diff and PR list of every PR tab in the strip, whose built-from head the
// stale mark compares with the list's.
if (flagOn) {
  app.overlaySnapshot((raw) => diffResource.overlay(raw));
  app.routeCmds((command, args) => diffResource.route(command, args));
  $effect.root(() => {
    $effect(() => showDiff(tabsResource.activeKey));
    $effect(() => {
      const releases = tabsResource.prRows().flatMap(({ key, remote }) => [
        resources.show(diffKey(key)),
        resources.show({ kind: "prs", remote }),
      ]);
      return () => {
        for (const release of releases) release();
      };
    });
  });
}
