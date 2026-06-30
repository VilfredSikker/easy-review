import type { TriageSnapshot } from "$lib/types";

/**
 * File paths Triage flagged for review, in priority order, de-duplicated.
 *
 * Returns an empty array when there is no triage, the triage is stale
 * (generated against a different diff), or it surfaced no priority files.
 * Callers use the empty result to hide the "review triage files" quick action.
 */
export function triageRecommendedPaths(
  triage: TriageSnapshot | null | undefined,
): string[] {
  if (!triage || !triage.fresh) return [];
  const seen = new Set<string>();
  const paths: string[] = [];
  for (const pf of triage.priority_files) {
    if (pf.path && !seen.has(pf.path)) {
      seen.add(pf.path);
      paths.push(pf.path);
    }
  }
  return paths;
}
