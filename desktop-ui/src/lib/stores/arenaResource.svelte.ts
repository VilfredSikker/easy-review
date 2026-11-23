import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { untrack } from "svelte";
import { resourceClient } from "$lib/protocol/client";
import type { ArenaRunSummary } from "$lib/types/arena";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { JOBS, annotationsKey } from "./annotationsAdapter";
import { ArenaAdapter } from "./arenaAdapter";
import { app } from "./app.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

let runs = $state<ArenaRunSummary[]>([]);
/** Bumped by a write that adds or removes a run, so the list is read again. */
let runsWritten = $state(0);

/** The arena's commands and reads, through `er_send` and `er_fetch` with the flag on. */
export const arenaAdapter = new ArenaAdapter({
  flagOn,
  store: resources,
  client: resourceClient(),
  activeKey: () => tabsResource.activeKey,
  invoke,
  runsMoved: () => {
    runsWritten += 1;
  },
});

// With the flag on the run lists come from the resources: `jobs` holds the
// live runs, and the active tab's run list is read again whenever it moves,
// when the tab's content first publishes, and after a write that adds or
// removes a run.
if (flagOn) {
  app.overlaySnapshot((raw) => arenaAdapter.overlay(raw, runs));
  $effect.root(() => {
    $effect(() => resources.show(JOBS));
    $effect(() => {
      const key = tabsResource.activeKey;
      untrack(() => {
        runs = [];
      });
      if (key === null) return;
      $effect(() => {
        arenaAdapter.jobsVersion();
        void resources.get(annotationsKey(key))?.version;
        void runsWritten;
        void arenaAdapter.runs().then(
          (next) => {
            if (tabsResource.activeKey === key) runs = next;
          },
          () => {},
        );
      });
    });
  });
}

/**
 * Calls `handler` each time `jobs` moves, which is how a live run's progress
 * reaches the arena page with the flag on. Answers the release.
 */
export function onJobsMoved(
  handler: () => void,
  adapter: Pick<ArenaAdapter, "jobsVersion"> = arenaAdapter,
): () => void {
  return $effect.root(() => {
    $effect(() => {
      if (adapter.jobsVersion() === null) return;
      untrack(handler);
    });
  });
}

/** What carries a live run's progress to the arena page. */
export interface RunProgressFeed {
  flagOn: boolean;
  adapter: Pick<ArenaAdapter, "jobsVersion">;
  listen: (event: string, handler: () => void) => Promise<() => void>;
}

/**
 * Calls `onRevision` each time a live run may have moved: when `jobs` moves
 * with the flag on, and on the desktop revision without it. Answers the
 * release.
 */
export function followRunProgress(
  onRevision: () => void,
  feed: RunProgressFeed = { flagOn, adapter: arenaAdapter, listen },
): Promise<() => void> {
  if (feed.flagOn) return Promise.resolve(onJobsMoved(onRevision, feed.adapter));
  return feed.listen("er://revision", onRevision);
}
