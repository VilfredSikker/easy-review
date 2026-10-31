import type { FlatFinding, ThreadSnapshot } from "./types";

/** Per-kind annotation totals for the display menu and the Comments filter. */
export interface AnnotationCounts {
  comments: number;
  questions: number;
  notes: number;
  findingsUnresolved: number;
  findingsResolved: number;
}

/**
 * Counts are of what exists, not of what the current filters leave visible:
 * a toggle that shows its own total tells the viewer what flipping it would
 * reveal. Resolved findings arrive in their own array, so they are counted
 * from it as well as from the flag.
 */
export function annotationCounts(
  threads: ThreadSnapshot[] | undefined | null,
  findings: FlatFinding[] | undefined | null,
  resolvedFindings?: FlatFinding[] | null,
): AnnotationCounts {
  const counts: AnnotationCounts = {
    comments: 0,
    questions: 0,
    notes: 0,
    findingsUnresolved: 0,
    findingsResolved: 0,
  };
  for (const t of threads ?? []) {
    if (t.kind === "comment") counts.comments++;
    else if (t.kind === "question") counts.questions++;
    else if (t.kind === "note") counts.notes++;
  }
  for (const f of findings ?? []) {
    if (f.resolved) counts.findingsResolved++;
    else counts.findingsUnresolved++;
  }
  counts.findingsResolved += resolvedFindings?.length ?? 0;
  return counts;
}
