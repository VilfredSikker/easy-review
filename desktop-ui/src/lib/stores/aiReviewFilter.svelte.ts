import {
  ALL_REVIEWERS,
  coerceAgentFilter,
  uniqueAgentLabels,
  type AgentFilter,
} from "$lib/aiReviewAgents";
import type { FlatFinding } from "$lib/types";

class AiReviewFilterStore {
  filter = $state<AgentFilter>(ALL_REVIEWERS);
  private labelsKey = "";

  syncFromFindings(findings: FlatFinding[]): void {
    const labels = uniqueAgentLabels(findings);
    const key = labels.join("\0");
    if (key === this.labelsKey) return;
    this.labelsKey = key;
    this.filter = coerceAgentFilter(labels, this.filter);
  }
}

export const aiReviewFilter = new AiReviewFilterStore();
