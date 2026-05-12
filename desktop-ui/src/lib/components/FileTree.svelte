<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
</script>

<div class="w-64 border-r border-ink-500/40 bg-ink-850 flex flex-col overflow-hidden">
  {#if app.snapshot}
    <div class="flex-1 overflow-y-auto">
      {#each app.snapshot.files as file, i}
        {@const selected = i === app.snapshot.selected_file}
        <div
          class="relative flex items-center gap-1.5 px-3 py-[3px] cursor-pointer hover:bg-ink-800 {selected ? 'bg-ink-700' : ''}"
        >
          {#if selected}
            <span class="absolute left-0 top-0 bottom-0 w-[3px] bg-accent"></span>
          {/if}
          <span class="truncate flex-1 text-[13px] {file.reviewed ? 'text-ink-300 line-through' : 'text-ink-100'}">{file.path}</span>
          {#if file.additions > 0}
            <span class="text-[11px] font-mono text-add-fg shrink-0">+{file.additions}</span>
          {/if}
          {#if file.deletions > 0}
            <span class="text-[11px] font-mono text-del-fg shrink-0">-{file.deletions}</span>
          {/if}
        </div>
      {/each}
    </div>
    <div class="border-t border-ink-500/40 px-3 py-1.5 text-[11px] font-mono text-ink-300">
      {app.snapshot.reviewed_count} / {app.snapshot.total_count} reviewed
    </div>
  {:else}
    <div class="flex-1 flex items-center justify-center text-ink-300 text-sm">Loading…</div>
  {/if}
</div>
