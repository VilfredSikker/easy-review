// The diff overlay lays the file rows down first; the marks go over them.
import { diffResource } from "./diffResource.svelte";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { ReviewedAdapter, reviewedKey } from "./reviewedAdapter";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

/** The active tab's reviewed marks and counts, from the snapshot or `reviewed(tab)` by build. */
export const reviewedResource = new ReviewedAdapter({
  flagOn,
  store: resources,
  activeKey: () => tabsResource.activeKey,
  rowsHeld: () => diffResource.held() !== null,
});

function showReviewed(key: TabKey | null): (() => void) | undefined {
  return key === null ? undefined : resources.show(reviewedKey(key));
}

// With the flag on the page reads the marks and the count from
// `reviewed(tab)`, and the reviewed writes go through er_send keyed to the
// active tab.
if (flagOn) {
  app.overlaySnapshot((snap) => reviewedResource.overlay(snap));
  app.routeCmds((command, args) => reviewedResource.route(command, args));
  $effect.root(() => {
    $effect(() => showReviewed(tabsResource.activeKey));
  });
}
