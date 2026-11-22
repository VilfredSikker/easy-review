import type { Hunk } from "$lib/protocol/generated/Hunk";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import { resourceClient } from "$lib/protocol/client";
import { HunkCache } from "$lib/hunkCache";
import { HunkLoader, type HashedFile } from "$lib/hunkLoader";
import { diffKey } from "./diffAdapter";
import { resources } from "./resourceStore";

const cache = new HunkCache();
const client = resourceClient();
/** Moves when the cache gains hunks, so a view that read a missing hash reads again. */
let revision = $state(0);

const loader = new HunkLoader({
  cache,
  fetch: (keys) => client.fetch(keys),
  refetchDiff: (tab) => resources.recheck([diffKey(tab)]),
  changed: () => {
    revision += 1;
  },
  report: (e) => console.warn(`[hunks] ${e.kind}: ${e.message}`),
});

/** The hunks the page holds by hash, and the fetch of the ones it shows and lacks. Flag-on only. */
export const hunksResource = {
  get(hash: string): Hunk[] | undefined {
    void revision;
    return cache.get(hash);
  },
  load(tab: TabKey, files: readonly HashedFile[]): Promise<void> {
    return loader.load(tab, files);
  },
};
