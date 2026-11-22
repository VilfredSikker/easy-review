import type { Hunk } from "$lib/protocol/generated/Hunk";

/**
 * How many bytes of hunks the page keeps. A hash names the same hunks
 * forever, so a file the page shows again, in this tab or another, paints
 * from here without a round trip; the bound is what keeps a long session's
 * diffs from growing without end.
 */
export const HUNK_CACHE_BYTES = 48 * 1024 * 1024;

/** Two bytes a character, plus what each line and hunk carries beside its text. */
export function hunkBytes(hunks: readonly Hunk[]): number {
  let bytes = 0;
  for (const hunk of hunks) {
    bytes += 64 + hunk.header.length * 2;
    for (const line of hunk.lines) bytes += 48 + line.text.length * 2;
  }
  return bytes;
}

/** Hunks by the hash `diff(tab)` names for them, least recently used first out. */
export class HunkCache {
  /** Insertion order is use order: a read moves its entry to the end. */
  private readonly entries = new Map<string, { hunks: Hunk[]; bytes: number }>();
  private total = 0;

  constructor(private readonly limit = HUNK_CACHE_BYTES) {}

  get(hash: string): Hunk[] | undefined {
    const entry = this.entries.get(hash);
    if (!entry) return undefined;
    this.entries.delete(hash);
    this.entries.set(hash, entry);
    return entry.hunks;
  }

  has(hash: string): boolean {
    return this.entries.has(hash);
  }

  /** Keeps `hunks` under `hash`, dropping the least recently used until the total fits. */
  put(hash: string, hunks: Hunk[]): void {
    const held = this.entries.get(hash);
    if (held) {
      this.entries.delete(hash);
      this.total -= held.bytes;
    }
    const bytes = hunkBytes(hunks);
    this.entries.set(hash, { hunks, bytes });
    this.total += bytes;
    for (const [oldest, entry] of this.entries) {
      if (this.total <= this.limit || oldest === hash) break;
      this.entries.delete(oldest);
      this.total -= entry.bytes;
    }
  }

  get bytes(): number {
    return this.total;
  }

  get size(): number {
    return this.entries.size;
  }
}
