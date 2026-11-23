import { invoke } from "@tauri-apps/api/core";
import { resourceClient } from "$lib/protocol/client";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { JOBS } from "./annotationsAdapter";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { TasksAdapter } from "./tasksAdapter";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

/** The background tasks' list, stops and logs, through `jobs`, `er_send` and `er_fetch` with the flag on. */
export const tasksAdapter = new TasksAdapter({
  flagOn,
  store: resources,
  client: resourceClient(),
  activeKey: () => tabsResource.activeKey,
  invoke,
});

// With the flag on the page lists the host's tasks from `jobs`, and a stop
// of one goes through er_send.
if (flagOn) {
  app.overlaySnapshot((raw) => tasksAdapter.overlay(raw));
  app.routeCmds((command, args) => tasksAdapter.route(command, args));
  $effect.root(() => {
    $effect(() => resources.show(JOBS));
  });
}
