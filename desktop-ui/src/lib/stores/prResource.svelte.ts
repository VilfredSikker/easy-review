// The diff overlay lays the branch down first; the PR status is read for it.
import "./diffResource.svelte";
import { app } from "./app.svelte";
import { pageView } from "./pageView.svelte";
import { PrAdapter, prKey, prStatusKey, type HeldPrs } from "./prAdapter";
import { prsKey } from "./prsAdapter";
import { projectsResource } from "./projectsResource.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

function heldPrs(): HeldPrs[] {
  return projectsResource.remotes.flatMap((remote) => {
    const value = resources.get(prsKey(remote))?.value;
    return value?.kind === "prs" ? [{ remote, prs: value }] : [];
  });
}

/** The active tab's PR card, stack and GitHub status, from `pr(tab)` and `pr_status`. */
export const prResource = new PrAdapter({
  store: resources,
  activeKey: () => tabsResource.activeKey,
  activeTab: () => tabsResource.activeTab,
  repoRemote: (root) => projectsResource.projects.find((p) => p.root_path === root)?.remote ?? null,
  heldPrs,
});

// The card reads the active tab's `pr(tab)` and the status of the PR it
// shows, and its two refreshes go through er_send.
pageView.overlay((raw) => prResource.overlay(raw));
app.routeCmds((command) => prResource.route(command));
$effect.root(() => {
  $effect(() => {
    const key = tabsResource.activeKey;
    return key === null ? undefined : resources.show(prKey(key));
  });
  $effect(() => {
    const view = pageView.current;
    const target = prResource.target(view?.local_branch ?? view?.branch);
    return target === null ? undefined : resources.show(prStatusKey(target));
  });
});
