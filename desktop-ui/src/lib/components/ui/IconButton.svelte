<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    children: Snippet;
    onclick?: () => void;
    title?: string;
    active?: boolean;
    /** Reduce opacity for placeholder/disabled-looking states. */
    dim?: boolean;
  }

  const { children, onclick, title, active = false, dim = false }: Props = $props();
</script>

<button
  {onclick}
  {title}
  aria-label={title}
  class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center transition-colors {active ? 'text-accent bg-hover' : 'text-muted'} {dim ? 'opacity-40' : ''}"
>
  {@render children()}
</button>
