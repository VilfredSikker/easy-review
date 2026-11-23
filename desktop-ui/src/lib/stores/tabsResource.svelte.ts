import { invoke } from "@tauri-apps/api/core";
import { routePrefetches } from "$lib/prOpen";
import { nameShownTab } from "$lib/tabCommands";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { BrowserViewAdapter } from "./browserView";
import { tabIsStale } from "./diffAdapter";
import { resources } from "./resourceStore";
import { TabsAdapter } from "./tabsAdapter";

const flagOn = v1ServiceRequested();

/**
 * The open tabs, from the snapshot or the `tabs` resource by build. It reads
 * the App's snapshot: the diff overlay reads the active tab through this
 * adapter while `app.snapshot` computes, so reading that would loop.
 */
export const tabsResource = new TabsAdapter({
  flagOn,
  store: resources,
  snapshot: () => app.appSnapshot,
  stale: (row, isActive) => tabIsStale(resources, row, isActive),
});

// With the flag on the tab commands, the opens and the prefetches go through
// er_send; every other command keeps its path.
nameShownTab(() => tabsResource.activeKey);
if (flagOn) {
  app.routeCmds((command, args) => tabsResource.route(command, args));
  routePrefetches((command, args) => tabsResource.route(command, args) ?? invoke(command, args));
}

/** The active tab's review browser, from the snapshot or its `view` resource by build. */
export const browserView = new BrowserViewAdapter({
  flagOn,
  store: resources,
  snapshot: () => app.snapshot,
  activeKey: () => tabsResource.activeKey,
});
