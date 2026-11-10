<script lang="ts">
  import { app } from "$lib/stores/app.svelte";

  /**
   * Persistent while `config.toml` cannot be loaded. Settings re-reads and
   * adopts the file on open, so reopening the page is the reload path.
   */
  const message = $derived(app.snapshot?.config_load_error ?? null);
</script>

{#if message}
  <div
    class="flex shrink-0 flex-wrap items-center gap-2.5 border-b border-line bg-[color-mix(in_srgb,var(--color-error)_9%,var(--color-bg))] px-3.5 py-2 text-[12.5px] text-fg-2"
    role="alert"
    data-testid="config-load-banner"
  >
    <span class="font-semibold text-error">Config</span>
    <span class="min-w-0 flex-1">{message}</span>
    <span class="text-fg-3">Fix the file, then reopen Settings to reload it.</span>
  </div>
{/if}
