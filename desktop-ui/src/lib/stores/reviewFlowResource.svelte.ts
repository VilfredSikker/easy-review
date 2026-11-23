import { invoke } from "@tauri-apps/api/core";
import { resourceClient } from "$lib/protocol/client";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { JOBS } from "./annotationsAdapter";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { ReviewFlowAdapter } from "./reviewFlowAdapter";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

/** The review flows' controls and reads, through `er_send`, `er_fetch` and `jobs` with the flag on. */
export const reviewFlowAdapter = new ReviewFlowAdapter({
  flagOn,
  store: resources,
  client: resourceClient(),
  activeKey: () => tabsResource.activeKey,
  invoke,
});

// With the flag on the page reads the painted bucket's flow from `jobs`,
// and the flow controls go through er_send keyed to the active tab.
if (flagOn) {
  app.overlaySnapshot((raw) => reviewFlowAdapter.overlay(raw));
  app.routeCmds((command, args) => reviewFlowAdapter.route(command, args));
  $effect.root(() => {
    $effect(() => resources.show(JOBS));
  });
}
