<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
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

  const snapshot = $derived(app.snapshot);
  const selectedFile = $derived(snapshot?.files[snapshot.selected_file] ?? null);
  const treeHidden = $derived(!snapshot?.panels.tree);
</script>

<div class="flex-1 flex flex-col min-w-0 overflow-hidden">
  <!-- Sticky file header -->
  <div class="h-10 px-4 border-b border-ink-500/40 bg-ink-850 flex items-center gap-3 shrink-0">
    {#if treeHidden}
      <button
        class="p-1 text-ink-400 hover:text-ink-100 hover:bg-ink-700 rounded shrink-0"
        onclick={() => app.togglePanel("tree")}
        title="Show file tree"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z"/></svg>
      </button>
    {/if}

    {#if selectedFile}
      <span class="font-mono text-sm text-ink-100 truncate">{selectedFile.path}</span>
      <span class="font-mono text-xs text-add-fg shrink-0">+{selectedFile.additions}</span>
      <span class="font-mono text-xs text-del-fg shrink-0">-{selectedFile.deletions}</span>
      <div class="ml-auto flex items-center gap-1">
        <button
          class="px-2 py-1 text-xs text-ink-200 hover:bg-ink-700 rounded"
          onclick={() => invoke("open_in_editor")}
        >
          Open in editor
        </button>
        <button
          class="px-2 py-1 text-xs flex items-center gap-1 hover:bg-ink-700 rounded {selectedFile.reviewed ? 'text-add-fg' : 'text-ink-200'}"
          onclick={() =>
            app.cmd(selectedFile!.reviewed ? "unmark_reviewed" : "mark_reviewed", {
              file_idx: snapshot!.selected_file,
            })}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 13l4 4L19 7"/></svg>
          {selectedFile.reviewed ? "Unmark" : "Mark reviewed"}
        </button>
      </div>
    {:else}
      <span class="text-ink-400 text-sm">No file selected</span>
    {/if}
  </div>

  <!-- Diff scroll area -->
  <div class="flex-1 overflow-y-auto font-mono text-[13px] leading-[1.55]">
    {#if !snapshot}
      <div class="flex items-center justify-center h-full text-ink-300">Loading…</div>
    {:else if !selectedFile || selectedFile.hunks.length === 0}
      <div class="flex items-center justify-center h-full text-ink-300 text-sm">
        {selectedFile?.compacted ? "File compacted — press Enter to expand" : "No changes"}
      </div>
    {:else}
      {#each selectedFile.hunks as hunk}
        <div class="px-4 py-1 text-ink-300 bg-ink-850 border-b border-ink-500/40 text-[12px]">
          {hunk.header}
        </div>
        {#each hunk.lines as line}
          <div class="grid grid-cols-[40px_40px_1fr] cursor-pointer {lineClass(line.kind)} {line.kind === 'context' ? 'hover:bg-ink-800/50' : ''}">
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
