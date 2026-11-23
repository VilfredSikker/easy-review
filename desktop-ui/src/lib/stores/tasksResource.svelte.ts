import { invoke } from "@tauri-apps/api/core";
import { SvelteMap } from "svelte/reactivity";
import { resourceClient } from "$lib/protocol/client";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { JOBS } from "./annotationsAdapter";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { TasksAdapter } from "./tasksAdapter";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

/** The background tasks' runs, list, stops and logs, through `er_send`, `jobs` and `er_fetch` with the flag on. */
export const tasksAdapter = new TasksAdapter({
  flagOn,
  store: resources,
  client: resourceClient(),
  activeKey: () => tabsResource.activeKey,
  invoke,
  logs: new SvelteMap(),
  toast: (message) => app.showToast("success", message),
});

// With the flag on the page lists the tasks from `jobs`, and the runs go
// through er_send keyed to the active tab.
if (flagOn) {
  app.overlaySnapshot((raw) => tasksAdapter.overlay(raw));
  app.routeCmds((command, args) => tasksAdapter.route(command, args));
  $effect.root(() => {
    $effect(() => resources.show(JOBS));
  });
  // The painted tab's agent log reads its tasks' output, which `jobs`
  // leaves out, once a second.
  setInterval(() => void tasksAdapter.refreshLogs(app.appSnapshot), 1000);
}
