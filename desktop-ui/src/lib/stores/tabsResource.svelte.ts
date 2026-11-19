import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { TabsAdapter } from "./tabsAdapter";

/** The open tabs for the tab strip, from the snapshot or the `tabs` resource by build. */
export const tabsResource = new TabsAdapter({
  flagOn: v1ServiceRequested(),
  store: resources,
  snapshot: () => app.snapshot,
});
