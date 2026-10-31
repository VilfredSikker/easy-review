import { untrack } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import { fetchLineCommits, lineCommitsCurrent, type LineCommits } from "$lib/lineCommits";

/**
 * Per-file line attribution for the diff on screen. The diff view asks for the
 * files in its window; the rows and the file header read it back. Keyed by
 * path, and a reply counts only for the `cache_key` it was computed for, so a
 * refreshed file asks again instead of colouring lines it no longer has.
 */
const results = new SvelteMap<string, LineCommits>();
/** Requests in flight or failed, by `path\0cacheKey`. A failed request is not
 *  retried for the same content: the window effect would ask on every scroll. */
// eslint-disable-next-line svelte/prefer-svelte-reactivity -- request bookkeeping nothing renders; reactive, it would make the window effect depend on its own writes
const asked = new Set<string>();
/** Bumped by `clear`, so a reply for the previous diff is dropped. */
let generation = 0;

function get(path: string, cacheKey: string): LineCommits | null {
  const r = results.get(path);
  return r && lineCommitsCurrent(r, path, cacheKey) ? r : null;
}

function request(path: string, cacheKey: string, fetcher = fetchLineCommits): void {
  // Called from the diff view's window effect: reading `results` here must not
  // make that effect depend on the map this request is about to write.
  untrack(() => {
    if (get(path, cacheKey)) return;
    const key = `${path}\0${cacheKey}`;
    if (asked.has(key)) return;
    asked.add(key);
    const gen = generation;
    fetcher(path, cacheKey).then(
      (reply) => {
        if (gen !== generation || !lineCommitsCurrent(reply, path, cacheKey)) return;
        results.set(path, reply);
        asked.delete(key);
      },
      () => {},
    );
  });
}

function clear(): void {
  generation++;
  asked.clear();
  untrack(() => {
    if (results.size > 0) results.clear();
  });
}

export const lineCommitStore = {
  get,
  request,
  clear,
};
