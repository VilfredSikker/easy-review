<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    children: Snippet;
    /** Color of the leading dot. `null` = no dot. */
    dot?: string | null;
    /** Override text color. Defaults to `text-muted`. */
    textColor?: string;
  }

  const { children, dot = null, textColor = "text-muted" }: Props = $props();
</script>

<span class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-full bg-ink-650 border border-border text-[10px] font-mono {textColor}">
  {#if dot}
    <span class="w-1 h-1 rounded-full" style="background: {dot}"></span>
  {/if}
  {@render children()}
</span>
