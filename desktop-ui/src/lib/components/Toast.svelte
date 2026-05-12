<script lang="ts">
  interface Props { message: string | null; }
  const { message }: Props = $props();
</script>

{#if message}
  <div class="fixed bottom-10 left-1/2 -translate-x-1/2 bg-ink-700 text-ink-100 text-xs font-mono px-4 py-2 rounded shadow-lg border border-ink-500/40 pointer-events-none z-50 transition-opacity">
    {message}
  </div>
{/if}
