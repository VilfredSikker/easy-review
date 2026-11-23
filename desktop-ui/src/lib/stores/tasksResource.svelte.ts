import { SvelteMap } from "svelte/reactivity";
import { resourceClient } from "$lib/protocol/client";
import { JOBS } from "./annotationsAdapter";
import { app } from "./app.svelte";
import { pageView } from "./pageView.svelte";
import { resources } from "./resourceStore";
import { TasksAdapter } from "./tasksAdapter";
import { tabsResource } from "./tabsResource.svelte";

/** The background tasks' runs, list, stops and logs, through `er_send`, `jobs` and `er_fetch`. */
export const tasksAdapter = new TasksAdapter({
  store: resources,
  client: resourceClient(),
  activeKey: () => tabsResource.activeKey,
  logs: new SvelteMap(),
});

// The page lists the host's tasks from `jobs`, and the runs and stops go
// through er_send keyed to the active tab.
pageView.overlay((view) => tasksAdapter.overlay(view));
app.routeCmds((command, args) => tasksAdapter.route(command, args));
$effect.root(() => {
  $effect(() => resources.show(JOBS));
});
// The painted tab's agent log reads its tasks' output, which `jobs` leaves
// out, once a second.
setInterval(() => void tasksAdapter.refreshLogs(pageView.current), 1000);
