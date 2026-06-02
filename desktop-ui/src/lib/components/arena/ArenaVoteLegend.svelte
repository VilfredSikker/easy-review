<script lang="ts">
  import type { Vote } from "$lib/types/arena";
  import ArenaVoteIcon from "$lib/components/arena/ArenaVoteIcon.svelte";

  interface Props {
    compact?: boolean;
    class?: string;
  }

  const { compact = false, class: className = "" }: Props = $props();

  const items: { vote: Vote; label: string }[] = [
    { vote: "keep", label: "Keep" },
    { vote: "drop", label: "Drop" },
    { vote: "merge", label: "Merge" },
    { vote: "escalate", label: "Escalate" },
    { vote: "lower", label: "Lower" },
    { vote: "flag", label: "Flag" },
    { vote: "abstain", label: "Abstain" },
    { vote: "propose", label: "Propose" },
  ];
</script>

<div
  class="flex flex-wrap items-center gap-x-3 gap-y-1 {compact ? 'text-[9px]' : 'text-[10px]'} {className}"
  role="list"
  aria-label="Vote legend"
>
  {#each items as item}
    <span class="inline-flex items-center gap-1 text-[var(--arena-fg-muted)]" role="listitem">
      <ArenaVoteIcon vote={item.vote} size={compact ? 12 : 14} />
      <span>{item.label}</span>
    </span>
  {/each}
</div>
