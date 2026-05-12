<script lang="ts">
  import { app } from "$lib/stores/app.svelte";

  function lineClass(kind: string) {
    if (kind === "add") return "bg-add-bg";
    if (kind === "del") return "bg-del-bg";
    return "";
  }

  function gutterClass(kind: string) {
    if (kind === "add") return "bg-add-bg text-add-fg/60";
    if (kind === "del") return "bg-del-bg text-del-fg/60";
    return "text-ink-300/40";
  }

  const selectedFile = $derived(
    app.snapshot?.files[app.snapshot.selected_file] ?? null
  );
</script>

<div class="flex-1 flex flex-col min-w-0 overflow-hidden">
  <!-- Sticky file header -->
  {#if selectedFile}
    <div class="h-10 px-4 border-b border-ink-500/40 bg-ink-850 flex items-center gap-3 shrink-0">
      <span class="font-mono text-sm text-ink-100 truncate">{selectedFile.path}</span>
      <span class="font-mono text-xs text-add-fg shrink-0">+{selectedFile.additions}</span>
      <span class="font-mono text-xs text-del-fg shrink-0">-{selectedFile.deletions}</span>
    </div>
  {/if}

  <!-- Diff scroll area -->
  <div class="flex-1 overflow-y-auto font-mono text-[13px] leading-[1.55]">
    {#if !app.snapshot}
      <div class="flex items-center justify-center h-full text-ink-300">Loading…</div>
    {:else if !selectedFile || selectedFile.hunks.length === 0}
      <div class="flex items-center justify-center h-full text-ink-300 text-sm">
        {selectedFile?.compacted ? "File compacted — press Enter to expand" : "No changes"}
      </div>
    {:else}
      {#each selectedFile.hunks as hunk}
        <!-- Hunk header -->
        <div class="px-4 py-1 text-ink-300 bg-ink-850 border-b border-ink-500/40 text-[12px]">
          {hunk.header}
        </div>
        <!-- Diff rows -->
        {#each hunk.lines as line}
          <div class="grid grid-cols-[40px_40px_1fr] {lineClass(line.kind)}">
            <div class="text-right pr-2 text-[11px] select-none {gutterClass(line.kind)}">
              {line.old_num ?? ""}
            </div>
            <div class="text-right pr-2 text-[11px] select-none {gutterClass(line.kind)}">
              {line.new_num ?? ""}
            </div>
            <div class="px-3 {line.kind === 'add' ? 'text-add-fg' : line.kind === 'del' ? 'text-del-fg' : 'text-ink-100'}">
              {#if line.kind === "add"}
                <span class="text-add-fg">+</span>
              {:else if line.kind === "del"}
                <span class="text-del-fg">-</span>
              {:else}
                <span>&nbsp;</span>
              {/if}
              {#each line.spans as span}
                {#if span.color}
                  <span style="color: {span.color}">{span.text}</span>
                {:else}
                  {span.text}
                {/if}
              {/each}
            </div>
          </div>
        {/each}
      {/each}
    {/if}
  </div>
</div>
