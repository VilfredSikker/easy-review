import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { PrAdapter, prKey, prStatusKey, type HeldPrs } from "./prAdapter";
import { prsKey } from "./prsAdapter";
import { projectsResource } from "./projectsResource.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

function heldPrs(): HeldPrs[] {
  return projectsResource.remotes.flatMap((remote) => {
    const value = resources.get(prsKey(remote))?.value;
    return value?.kind === "prs" ? [{ remote, prs: value }] : [];
  });
}

/** The active tab's PR card, stack and GitHub status, from the snapshot or `pr(tab)` and `pr_status` by build. */
export const prResource = new PrAdapter({
  flagOn,
  store: resources,
  activeKey: () => tabsResource.activeKey,
  activeTab: () => tabsResource.activeTab,
  repoRemote: (root) => projectsResource.projects.find((p) => p.root_path === root)?.remote ?? null,
  heldPrs,
});

// With the flag on the card reads the active tab's `pr(tab)` and the status
// of the PR it shows, and its two refreshes go through er_send.
if (flagOn) {
  app.overlaySnapshot((raw) => prResource.overlay(raw));
  app.routeCmds((command) => prResource.route(command));
  $effect.root(() => {
    $effect(() => {
      const key = tabsResource.activeKey;
      return key === null ? undefined : resources.show(prKey(key));
    });
    $effect(() => {
      const raw = app.appSnapshot;
      const target = prResource.target(raw?.local_branch ?? raw?.branch);
      return target === null ? undefined : resources.show(prStatusKey(target));
    });
  });
}
