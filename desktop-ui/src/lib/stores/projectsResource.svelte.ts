import { invoke } from "@tauri-apps/api/core";
import { resourceClient } from "$lib/protocol/client";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { ProjectsAdapter } from "./projectsAdapter";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

/** The sidebar's projects, from the snapshot or the sidebar resources by build. */
export const projectsResource = new ProjectsAdapter({
  flagOn: v1ServiceRequested(),
  store: resources,
  client: resourceClient(),
  snapshot: () => app.snapshot,
  activeTab: () => tabsResource.tabs.find((tab) => tab.is_active) ?? null,
  command: (name, args) => app.cmd(name, args),
  invoke,
  report: (command, e) => app.showToast("error", `${command}: ${e.message}`),
});

// Each listed project's branches are shown while the list names it.
$effect.root(() => {
  $effect(() => projectsResource.showBranches(projectsResource.projectIds));
});
