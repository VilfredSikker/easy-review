import type { AiModelInfo, AiProviderInfo } from "$lib/types";
import type { ReviewerRef } from "$lib/types/arena";

export function formatModelPricePer1k(m: AiModelInfo): string {
  const cin = m.cost_per_1k_in ?? 0.003;
  const cout = m.cost_per_1k_out ?? 0.015;
  return `$${(cin + cout).toFixed(3)}/1k`;
}

export function estimateArenaCost(
  providers: AiProviderInfo[],
  picked: ReviewerRef[],
  rounds: number,
): { latencySec: number; costUsd: string } {
  let maxLatency = 0;
  let cost = 0;
  for (const ref of picked) {
    const p = providers.find((x) => x.id === ref.provider_id);
    const m = p?.models.find((x) => x.id === ref.model_id);
    const lat = m?.avg_latency_ms ?? 12_000;
    maxLatency = Math.max(maxLatency, lat);
    const cin = m?.cost_per_1k_in ?? 0.003;
    const cout = m?.cost_per_1k_out ?? 0.015;
    cost += (cin + cout) * 8 * rounds;
  }
  const latencySec = Math.round((maxLatency * rounds * 0.85) / 1000);
  return {
    latencySec: Math.max(5, latencySec),
    costUsd: `$${cost.toFixed(2)}`,
  };
}
