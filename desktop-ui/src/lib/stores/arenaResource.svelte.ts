import { invoke } from "@tauri-apps/api/core";
import { untrack } from "svelte";
import { resourceClient } from "$lib/protocol/client";
import type { ArenaRunSummary } from "$lib/types/arena";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { JOBS } from "./annotationsAdapter";
import { ArenaAdapter } from "./arenaAdapter";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

/** The arena's commands and reads, through `er_send` and `er_fetch` with the flag on. */
export const arenaAdapter = new ArenaAdapter({
  flagOn,
  store: resources,
  client: resourceClient(),
  activeKey: () => tabsResource.activeKey,
  invoke,
});

let runs = $state<ArenaRunSummary[]>([]);

// With the flag on the run lists come from the resources: `jobs` holds the
// live runs, and the active tab's run list is read again whenever it moves.
if (flagOn) {
  app.overlaySnapshot((raw) => arenaAdapter.overlay(raw, runs));
  $effect.root(() => {
    $effect(() => resources.show(JOBS));
    $effect(() => {
      const key = tabsResource.activeKey;
      arenaAdapter.jobsVersion();
      if (key === null) return;
      void arenaAdapter.runs().then(
        (next) => {
          if (tabsResource.activeKey === key) runs = next;
        },
        () => {},
      );
    });
  });
}

/**
 * Calls `handler` each time `jobs` moves, which is how a live run's progress
 * reaches the arena page with the flag on. Answers the release.
 */
export function onJobsMoved(handler: () => void): () => void {
  return $effect.root(() => {
    $effect(() => {
      if (arenaAdapter.jobsVersion() === null) return;
      untrack(handler);
    });
  });
}
