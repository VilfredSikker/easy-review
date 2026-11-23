// The marks are laid down first; a jump to an unreviewed file reads them.
import "./reviewedResource.svelte";
import { app } from "./app.svelte";
import { diffResource } from "./diffResource.svelte";
import { pageView } from "./pageView.svelte";
import { SelectionAdapter } from "./selectionAdapter";
import { tabsResource } from "./tabsResource.svelte";

/** The active tab's selected file and hunk, which the page owns. */
export const selection = new SelectionAdapter({
  activeKey: () => tabsResource.activeKey,
  drawsFromDiff: () => diffResource.hashed(),
  shown: () => pageView.current,
});

pageView.overlay((snap) => selection.overlay(snap));
app.routeCmds((command, args) => selection.route(command, args));
