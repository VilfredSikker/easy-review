// The marks are laid down first; a jump to an unreviewed file reads them.
import "./reviewedResource.svelte";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { diffResource } from "./diffResource.svelte";
import { SelectionAdapter } from "./selectionAdapter";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

/** The active tab's selected file and hunk: the App's with the flag off, the page's with it on. */
export const selection = new SelectionAdapter({
  flagOn,
  activeKey: () => tabsResource.activeKey,
  drawsFromDiff: () => diffResource.hashed(),
  shown: () => app.snapshot,
});

if (flagOn) {
  app.overlaySnapshot((snap) => selection.overlay(snap));
  app.routeCmds((command, args) => selection.route(command, args));
}
