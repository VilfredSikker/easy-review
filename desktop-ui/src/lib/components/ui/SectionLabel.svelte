<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    children: Snippet;
    /** "lg" = `text-xs` (default — card headers). "sm" = `text-[10px]` (sub-section headers). */
    size?: "lg" | "sm";
  }

  const { children, size = "lg" }: Props = $props();
</script>

{#if size === "sm"}
  <span class="text-[10px] uppercase tracking-wider text-muted">{@render children()}</span>
{:else}
  <span class="text-xs uppercase tracking-wider text-muted">{@render children()}</span>
{/if}
