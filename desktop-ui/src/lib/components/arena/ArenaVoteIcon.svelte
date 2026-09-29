<script lang="ts">
  import type { Vote } from "$lib/types/arena";

  interface Props {
    vote: Vote;
    size?: number;
    class?: string;
    title?: string;
  }

  const { vote, size = 16, class: className = "", title = "" }: Props = $props();

  const VOTE_TONE: Record<Vote, string> = {
    keep: "text-[var(--arena-ok)]",
    drop: "text-[var(--arena-fg-faint)]",
    escalate: "text-[var(--arena-err)]",
    merge: "text-[var(--arena-periwinkle)]",
    lower: "text-[var(--arena-warn)]",
    flag: "text-[var(--arena-warn)]",
    propose: "text-[var(--arena-orange)]",
    abstain: "text-[var(--arena-fg-muted)]",
  };

  const tone = $derived(VOTE_TONE[vote] ?? "text-[var(--arena-fg-muted)]");
</script>

<span
  class="inline-flex items-center justify-center {tone} {className}"
  style="width:{size}px;height:{size}px"
  {title}
  aria-hidden={title ? undefined : true}
>
  {#if vote === "keep"}
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M3 8.5l3 3 7-7" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  {:else if vote === "drop"}
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M4 4l8 8M12 4l-8 8" stroke-linecap="round" />
    </svg>
  {:else if vote === "merge"}
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8">
      <circle cx="8" cy="8" r="6" />
      <path d="M8 5v6M5 8h6" stroke-linecap="round" />
    </svg>
  {:else if vote === "escalate"}
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor">
      <path d="M8 3l5 6H3l5-6zm0 10V8h0" opacity="0" />
      <path d="M8 2l6 8H2l6-8z" />
    </svg>
  {:else if vote === "lower"}
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor">
      <path d="M8 14L2 6h12L8 14z" />
    </svg>
  {:else if vote === "flag"}
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor">
      <path d="M4 2v12M4 2h7l-1.5 3L11 8H4" />
    </svg>
  {:else if vote === "propose"}
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M8 3v10M3 8h10" stroke-linecap="round" />
    </svg>
  {:else}
    <span class="text-[12px] leading-none">·</span>
  {/if}
</span>
