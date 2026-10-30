<script lang="ts">
  import { isRiskTier, tierLabel } from "./riskRules";

  interface Props {
    tier: string;
    readable?: boolean;
  }

  const { tier, readable = true }: Props = $props();

  // One hue per tier, from the theme's risk tokens; `ai` takes the purple the
  // rest of the app uses for AI-judged state.
  const TIER_CLASS: Record<string, { text: string; mark: string }> = {
    high: { text: "text-risk-high", mark: "bg-risk-high" },
    med: { text: "text-risk-med", mark: "bg-risk-med" },
    low: { text: "text-risk-low", mark: "bg-risk-low" },
    ai: { text: "text-periwinkle", mark: "bg-periwinkle" },
  };

  const tone = $derived(readable && isRiskTier(tier) ? TIER_CLASS[tier] : null);
</script>

{#if tone}
  <span class="inline-flex items-center gap-1.5 text-[11px] font-medium {tone.text}">
    <i class="w-[7px] h-[7px] rounded-[2px] {tone.mark}" aria-hidden="true"></i>{tierLabel(tier)}
  </span>
{:else}
  <span
    class="inline-flex items-center gap-1.5 text-[11px] font-mono text-risk-high line-through"
    title="Not a tier: this rule claims nothing until it reads high, med, low or ai"
  >{tier || "?"}</span>
{/if}
