import type { ErError } from "$lib/protocol/generated/ErError";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { Versioned } from "$lib/protocol/generated/Versioned";
import type { HunkCache } from "./hunkCache";

/** A file the view asks hunks for: its path and the hash its `diff(tab)` row names. */
export interface HashedFile {
  path: string;
  hash: string;
}

interface HunkLoaderDeps {
  cache: HunkCache;
  fetch: (keys: ResourceKey[]) => Promise<Versioned<Resource>[]>;
  /** Asks for `diff(tab)` again, when a hash the page held named nothing. */
  refetchDiff: (tab: TabKey) => void;
  /** Called after the cache gained hunks. */
  changed: () => void;
  report?: (e: ErError) => void;
}

/**
 * Fetches the hunks the view shows and does not hold, in one `er_fetch` per
 * call, so a scroll that reveals several files is one round trip. A `Stale`
 * answer means the diff moved under the page: the page fetches `diff(tab)`
 * again, and its new hashes come back through the view.
 */
export class HunkLoader {
  private readonly inFlight = new Set<string>();

  constructor(private readonly deps: HunkLoaderDeps) {}

  async load(tab: TabKey, files: readonly HashedFile[]): Promise<void> {
    const asked = files.filter((f) => !this.deps.cache.has(f.hash) && !this.inFlight.has(f.hash));
    if (asked.length === 0) return;
    for (const f of asked) this.inFlight.add(f.hash);
    try {
      const keys: ResourceKey[] = asked.map(({ path, hash }) => ({ kind: "hunks", tab, path, hash }));
      const answers = await this.deps.fetch(keys);
      answers.forEach((answer, i) => {
        if (answer.value.kind !== "hunks") return;
        // A lazy file was parsed to answer: keep it under the hash the next
        // diff names as well as the one asked.
        this.deps.cache.put(asked[i].hash, answer.value.hunks);
        if (answer.value.hash !== asked[i].hash) this.deps.cache.put(answer.value.hash, answer.value.hunks);
      });
      this.deps.changed();
    } catch (e) {
      const error = e as ErError;
      if (error.kind === "stale") this.deps.refetchDiff(tab);
      else this.deps.report?.(error);
    } finally {
      for (const f of asked) this.inFlight.delete(f.hash);
    }
  }
}
