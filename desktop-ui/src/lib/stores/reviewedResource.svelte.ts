// The diff overlay lays the file rows down first; the marks go over them.
import { diffResource } from "./diffResource.svelte";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import { app } from "./app.svelte";
import { pageView } from "./pageView.svelte";
import { ReviewedAdapter, reviewedKey } from "./reviewedAdapter";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

/** The active tab's reviewed marks and counts, from `reviewed(tab)`. */
export const reviewedResource = new ReviewedAdapter({
  store: resources,
  activeKey: () => tabsResource.activeKey,
  rowsHeld: () => diffResource.held() !== null,
});

function showReviewed(key: TabKey | null): (() => void) | undefined {
  return key === null ? undefined : resources.show(reviewedKey(key));
}

// The page reads the marks and the count from `reviewed(tab)`, and the
// reviewed writes go through er_send keyed to the active tab.
pageView.overlay((snap) => reviewedResource.overlay(snap));
app.routeCmds((command, args) => reviewedResource.route(command, args));
$effect.root(() => {
  $effect(() => showReviewed(tabsResource.activeKey));
});
