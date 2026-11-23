// The diff overlay sets the painted bucket first; the flow is read for it.
import "./diffResource.svelte";
import { resourceClient } from "$lib/protocol/client";
import { JOBS } from "./annotationsAdapter";
import { app } from "./app.svelte";
import { pageView } from "./pageView.svelte";
import { resources } from "./resourceStore";
import { ReviewFlowAdapter } from "./reviewFlowAdapter";
import { tabsResource } from "./tabsResource.svelte";

/** The review flows' controls and reads, through `er_send`, `er_fetch` and `jobs`. */
export const reviewFlowAdapter = new ReviewFlowAdapter({
  store: resources,
  client: resourceClient(),
  activeKey: () => tabsResource.activeKey,
});

// The page reads the painted bucket's flow from `jobs`, and the flow
// controls go through er_send keyed to the active tab.
pageView.overlay((raw) => reviewFlowAdapter.overlay(raw));
app.routeCmds((command, args) => reviewFlowAdapter.route(command, args));
$effect.root(() => {
  $effect(() => resources.show(JOBS));
});
