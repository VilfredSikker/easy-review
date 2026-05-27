import type { FlatFinding } from "$lib/types";

export type FindingSeverityFilter = "all" | FlatFinding["severity"];

class AiFindingFilterStore {
  severity = $state<FindingSeverityFilter>("all");

  setSeverity(next: FindingSeverityFilter) {
    this.severity = next;
  }
}

export function findingMatchesSeverity(
  finding: FlatFinding,
  filter: FindingSeverityFilter,
): boolean {
  return filter === "all" || finding.severity === filter;
}

export const aiFindingFilter = new AiFindingFilterStore();
