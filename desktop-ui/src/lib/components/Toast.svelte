<script lang="ts">
  interface Props { message: string | null; }
  const { message }: Props = $props();
</script>

{#if message}
  <div class="fixed right-6 bottom-6 bg-ink-700 text-ink-100 text-xs font-mono w-80 min-h-28 px-4 py-3 rounded-none shadow-lg border border-ink-500/40 pointer-events-none z-50 transition-opacity whitespace-pre-wrap break-words leading-relaxed">
    {message}
  </div>
{/if}
