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
/** The newest `cacheKey` requested per path. Replies can finish in any order;
 *  an older one landing last would replace the current result, and no effect
 *  would ask again, so it is dropped. */
// eslint-disable-next-line svelte/prefer-svelte-reactivity -- request bookkeeping nothing renders, same as `asked`
const latest = new Map<string, string>();
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
    latest.set(path, cacheKey);
    const gen = generation;
    fetcher(path, cacheKey).then(
      (reply) => {
        if (gen !== generation) return;
        if (latest.get(path) !== cacheKey) {
          // Superseded, not failed: the file may come back to this content.
          asked.delete(key);
          return;
        }
        if (!lineCommitsCurrent(reply, path, cacheKey)) return;
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
  latest.clear();
  untrack(() => {
    if (results.size > 0) results.clear();
  });
}

export const lineCommitStore = {
  get,
  request,
  clear,
};
