<script module lang="ts">
  /** `neutral` is metadata; the rest borrow the status tokens so the tint says what the word means. */
  export type PillTone =
    | "neutral"
    | "info"
    | "success"
    | "warning"
    | "risk-high"
    | "risk-med"
    | "risk-low";
</script>

<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    children: Snippet;
    tone?: PillTone;
    /** Color of the leading dot. `null` = no dot. */
    dot?: string | null;
    title?: string;
  }

  const { children, tone = "neutral", dot = null, title }: Props = $props();

  const TONE_CLASS: Record<PillTone, string> = {
    neutral: "border-border bg-hairline text-muted",
    info: "border-info/30 bg-info/10 text-info",
    success: "border-success/30 bg-success/10 text-success",
    warning: "border-warning/30 bg-warning/10 text-warning",
    "risk-high": "border-risk-high/30 bg-risk-high/10 text-risk-high",
    "risk-med": "border-risk-med/30 bg-risk-med/10 text-risk-med",
    "risk-low": "border-risk-low/30 bg-risk-low/10 text-risk-low",
  };

  const toneClass = $derived(TONE_CLASS[tone]);
</script>

<span
  class="inline-flex items-center gap-1 whitespace-nowrap rounded-full border px-1.5 py-0.5 font-mono text-[10px] {toneClass}"
  {title}
>
  {#if dot}
    <span class="h-1 w-1 rounded-full" style="background: {dot}"></span>
  {/if}
  {@render children()}
</span>
