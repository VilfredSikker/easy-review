import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot, RiskFilesView, RiskRulesView } from "$lib/types";

/**
 * The repo the risk commands answer for. The caller names it, so the backend
 * never reads the active tab: the tab's root, plus the PR's repo only for a
 * remote-only tab (a clone is asked for its own remote).
 */
export interface RiskTarget {
  repoRoot: string;
  remoteRepo: string | null;
}

export function riskTarget(snapshot: AppSnapshot | null): RiskTarget | null {
  const tab = snapshot?.tabs.find((t) => t.is_active);
  if (!tab) return null;
  return { repoRoot: tab.repo_root, remoteRepo: tab.kind === "remote_pr" ? tab.remote : null };
}

/** The active tab's changed files, which the rule table counts and resolves. */
export function changedPaths(snapshot: AppSnapshot | null): string[] {
  return snapshot?.files.map((f) => f.path) ?? [];
}

export function getRiskRules(target: RiskTarget): Promise<RiskRulesView> {
  return invoke<RiskRulesView>("get_risk_rules", { ...target });
}

export function getRiskFiles(target: RiskTarget, paths: string[]): Promise<RiskFilesView> {
  return invoke<RiskFilesView>("get_risk_files", { ...target, paths });
}
