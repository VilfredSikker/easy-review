import type { SpanSnapshot } from "$lib/types";

export interface HunkHighlight {
  hunk_index: number;
  lines: SpanSnapshot[][];
}

interface CacheEntry {
  generation: number;
  hunks: HunkHighlight[];
}

/** Bump when highlight stitch / span-mapping logic changes (invalidates stale entries). */
const HIGHLIGHT_CACHE_VERSION = "v4";

/** LRU cache for file highlight results, keyed by "filePath::cacheKey::syntaxTheme". */
class HighlightCache {
  private map = new Map<string, CacheEntry>();
  private generation = 0;
  private capacity: number;

  constructor(capacity = 50) {
    this.capacity = capacity;
  }

  key(filePath: string, cacheKey: string, syntaxTheme: string): string {
    return `${filePath}::${cacheKey}::${syntaxTheme}::${HIGHLIGHT_CACHE_VERSION}`;
  }

  get(k: string): HunkHighlight[] | undefined {
    const entry = this.map.get(k);
    if (!entry) return undefined;
    this.generation++;
    entry.generation = this.generation;
    return entry.hunks;
  }

  set(k: string, hunks: HunkHighlight[]): void {
    if (this.map.size >= this.capacity && !this.map.has(k)) {
      let lruKey: string | undefined;
      let lruGen = Infinity;
      for (const [ek, ev] of this.map) {
        if (ev.generation < lruGen) {
          lruGen = ev.generation;
          lruKey = ek;
        }
      }
      if (lruKey !== undefined) this.map.delete(lruKey);
    }
    this.generation++;
    this.map.set(k, { generation: this.generation, hunks });
  }

  has(k: string): boolean {
    return this.map.has(k);
  }

  delete(k: string): boolean {
    return this.map.delete(k);
  }
}

export const highlightCache = new HighlightCache(50);
