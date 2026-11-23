import { untrack } from "svelte";
import { resourceClient } from "$lib/protocol/client";
import type { ArenaRunSummary } from "$lib/types/arena";
import { JOBS, annotationsKey } from "./annotationsAdapter";
import { ArenaAdapter } from "./arenaAdapter";
import { pageView } from "./pageView.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

let runs = $state<ArenaRunSummary[]>([]);
/** Bumped by a write that adds or removes a run, so the list is read again. */
let runsWritten = $state(0);

/** The arena's commands and reads, through `er_send` and `er_fetch`. */
export const arenaAdapter = new ArenaAdapter({
  store: resources,
  client: resourceClient(),
  activeKey: () => tabsResource.activeKey,
  runsMoved: () => {
    runsWritten += 1;
  },
});

// The run lists come from the resources: `jobs` holds the
// live runs, and the active tab's run list is read again whenever it moves,
// when the tab's content first publishes, and after a write that adds or
// removes a run.
pageView.overlay((raw) => arenaAdapter.overlay(raw, runs));
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

/**
 * Calls `handler` each time `jobs` moves, which is how a live run's progress
 * reaches the arena page. Answers the release.
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

/** Calls `onRevision` each time a live run may have moved, which `jobs` moving says. Answers the release. */
export function followRunProgress(
  onRevision: () => void,
  adapter: Pick<ArenaAdapter, "jobsVersion"> = arenaAdapter,
): () => void {
  return onJobsMoved(onRevision, adapter);
}
