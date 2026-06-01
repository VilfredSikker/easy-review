import type { AiModelInfo, AiProviderInfo } from "$lib/types";
import type { ReviewerRef } from "$lib/types/arena";

export function formatModelPricePer1k(m: AiModelInfo): string {
  const cin = m.cost_per_1k_in ?? 0.003;
  const cout = m.cost_per_1k_out ?? 0.015;
  return `$${(cin + cout).toFixed(3)}/1k`;
}

export function modelCostPer1k(m: AiModelInfo): number {
  return (m.cost_per_1k_in ?? 0.003) + (m.cost_per_1k_out ?? 0.015);
}

/** Default arbiter: highest combined in/out cost across all hub models. */
export function pickMostExpensiveModel(providers: AiProviderInfo[]): ReviewerRef | null {
  let best: { cost: number; ref: ReviewerRef } | null = null;
  for (const p of providers) {
    for (const m of p.models) {
      const cost = modelCostPer1k(m);
      const ref = { provider_id: p.id, model_id: m.id };
      if (!best || cost > best.cost) {
        best = { cost, ref };
      }
    }
  }
  return best?.ref ?? null;
}

export function modelLabel(providers: AiProviderInfo[], ref: ReviewerRef): string {
  for (const p of providers) {
    if (p.id !== ref.provider_id) continue;
    const m = p.models.find((x) => x.id === ref.model_id);
    if (m) return m.label;
  }
  return ref.model_id;
}
