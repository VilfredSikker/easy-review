import { agentCatalogEntry } from "$lib/arena/agents";
import type { ArenaRunSnapshot } from "$lib/types/arena";
import type { ArenaStartConfig } from "$lib/stores/arena.svelte";

/** 2+ models on the same flow → AI Arena. */
export function isArenaRunFromConfig(config: ArenaStartConfig | null): boolean {
  if (!config) return false;
  if (config.mode === "agents" && config.agent_groups?.length) {
    return config.agent_groups.some((g) => g.models.length >= 2);
  }
  return (config.reviewers?.length ?? 0) >= 2;
}

export function isArenaRunFromSnapshot(snap: ArenaRunSnapshot): boolean {
  return snap.run.reviewers.length >= 2;
}

/** One model, one flow → single review (Review tab, not Arena overlay). */
export function isSingleReviewRun(
  config: ArenaStartConfig | null,
  snap: ArenaRunSnapshot | null,
): boolean {
  if (snap) {
    return snap.run.reviewers.length === 1;
  }
  if (!config) return false;
  if (config.mode === "agents" && config.agent_groups?.length) {
    return config.agent_groups.every((g) => g.models.length === 1);
  }
  return (config.reviewers?.length ?? 0) === 1;
}

/** Review-tab dropdown label for `run.config.agent_kind` (matches Rust `agent_label_for_category`). */
export function agentLabelFromSnapshot(snap: ArenaRunSnapshot): string {
  const kind = snap.run.config.agent_kind;
  if (kind) {
    return agentCatalogEntry(kind, kind, "").label;
  }
  const name = snap.run.reviewers[0]?.name;
  return name ?? "General";
}
