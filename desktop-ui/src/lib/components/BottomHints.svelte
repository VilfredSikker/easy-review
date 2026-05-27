<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
</script>

<div class="h-7 border-t border-hairline bg-ink-870 px-4 flex items-center gap-3 text-[11px] text-muted mono whitespace-nowrap overflow-hidden shrink-0">
  <span><span class="kbd">j</span><span class="kbd">k</span> <span class="ml-1">nav</span></span>
  <span><span class="kbd">/</span> <span class="ml-1">search</span></span>
  <span><span class="kbd">e</span> <span class="ml-1">VS Code</span></span>
  <span><span class="kbd">⌘R</span> <span class="ml-1">reload</span></span>
  <span class="text-ink-400">·</span>
  <span><span class="kbd">⌘K</span> <span class="ml-1">all shortcuts</span></span>
  {#if app.snapshot}
    <span class="ml-auto truncate">{app.snapshot.base} ← {app.snapshot.branch}</span>
  {/if}
</div>
