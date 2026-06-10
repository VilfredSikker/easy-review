import type { AppSnapshot, FileSnapshot } from "$lib/types";

export interface ResolveOmittedStats {
  /** Files whose hunks were spliced in from the previous snapshot. */
  reused: number;
  /** Omitted files we held no matching content for — downgraded to lazy
   * stubs so the viewport loader re-fetches them. */
  refetch: number;
}

/**
 * Resolve differential snapshots: the backend omits hunk payloads for files
 * whose content we already hold (`hunks_omitted` + matching `delta_key`).
 * Splice the previous snapshot's hunks into `next` in place. Must run on
 * every snapshot that replaces `app.snapshot` wholesale, BEFORE it is stored
 * — stored snapshots never carry `hunks_omitted`.
 *
 * Self-healing: when no matching content exists (first load, races between
 * concurrent commands, dropped snapshots), the file becomes a lazy stub and
 * the existing viewport-driven `request_file_content` path re-fetches it.
 */
export function resolveOmittedHunks(
  prev: AppSnapshot | null,
  next: AppSnapshot,
): ResolveOmittedStats {
  const stats: ResolveOmittedStats = { reused: 0, refetch: 0 };
  if (!next.files?.length) return stats;

  let prevByPath: Map<string, FileSnapshot> | null = null;
  for (const f of next.files) {
    if (!f.hunks_omitted) continue;
    f.hunks_omitted = false;
    if (prevByPath === null) {
      prevByPath = new Map();
      for (const p of prev?.files ?? []) prevByPath.set(p.path, p);
    }
    const p = prevByPath.get(f.path);
    if (
      p &&
      !p.is_lazy_stub &&
      p.hunks.length > 0 &&
      p.delta_key !== undefined &&
      p.delta_key === f.delta_key
    ) {
      f.hunks = p.hunks;
      stats.reused++;
    } else {
      f.hunks = [];
      f.is_lazy_stub = true;
      stats.refetch++;
    }
  }
  return stats;
}
