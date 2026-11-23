import { invoke } from "@tauri-apps/api/core";
import { routePrefetches } from "$lib/prOpen";
import { nameShownTab } from "$lib/tabCommands";
import { app } from "./app.svelte";
import { BrowserViewAdapter } from "./browserView";
import { tabIsStale } from "./diffAdapter";
import { legacySnapshot } from "./legacySnapshot.svelte";
import { pageView } from "./pageView.svelte";
import { resources } from "./resourceStore";
import { TabsAdapter } from "./tabsAdapter";

/** The open tabs, from the `tabs` resource. */
export const tabsResource = new TabsAdapter({
  store: resources,
  rows: () => legacySnapshot.tabRows,
  stale: (row, isActive) => tabIsStale(resources, row, isActive),
});

// The tab commands, the opens and the prefetches go through er_send; every
// other command keeps its path.
nameShownTab(() => tabsResource.activeKey);
app.routeCmds((command, args) => tabsResource.route(command, args));
routePrefetches((command, args) => tabsResource.route(command, args) ?? invoke(command, args));

// The page's view starts from the tab strip, once it has arrived.
pageView.readTabs(() => (tabsResource.loaded ? { tabs: tabsResource.tabs, active_tab: tabsResource.active } : null));

/** The active tab's review browser, from its `view` resource. */
export const browserView = new BrowserViewAdapter({
  store: resources,
  activeKey: () => tabsResource.activeKey,
});
