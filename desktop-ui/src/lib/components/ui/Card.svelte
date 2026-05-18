<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    children: Snippet;
    class?: string;
  }

  const { children, class: extra = "" }: Props = $props();
</script>

<div class="bg-card border border-hairline rounded-xl p-4 {extra}">
  {@render children()}
</div>
