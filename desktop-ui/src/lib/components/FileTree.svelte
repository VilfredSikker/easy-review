<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { diffScroll } from "$lib/stores/diffScroll.svelte";
  import { buildTree } from "$lib/treeFromPaths";
  import ScopeSelector from "./ScopeSelector.svelte";

  interface Props {
    /** When true, render narrow icon-only rail (mock lines 414–421). */
    collapsed?: boolean;
  }
  const { collapsed = false }: Props = $props();

  const snapshot = $derived(app.snapshot);
  const files = $derived(snapshot?.files ?? []);
  const ai = $derived(snapshot?.ai);
  const tree = $derived(buildTree(files));
  const selectedFile = $derived(snapshot ? files[snapshot.selected_file] : null);
  /** Driven by the diff viewport's IntersectionObserver — overrides the
   *  selected-file cursor for the tree highlight so the highlight tracks scroll. */
  const viewportPath = $derived(diffScroll.currentFilePath);

  function jumpToFile(path: string, idx: number) {
    // Smooth-scroll the diff view to this file's anchor section.
    document.getElementById(`file-${path}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
    // Keep the focus cursor in sync for keyboard nav / Mark reviewed.
    app.cmd("select_file", { idx });
  }

  /** `pl-3` (base) + 16px per additional depth, mirroring the mock's pl-7 → pl-11 progression. */
  function indentPx(depth: number): string {
    return `${12 + depth * 16}px`;
  }

  function findingColor(risk: string | null): string {
    if (risk === "high") return "#ef4444";
    if (risk === "med") return "#fbbf24";
    if (risk === "low") return "#60a5fa";
    return "#5e5e5e";
  }
</script>

{#if collapsed}
  <!-- Collapsed rail (mock lines 414–421) -->
  <div class="w-10 border-r border-hairline bg-surface flex flex-col items-center py-3 gap-2 transition-[width] duration-200">
    <button
      onclick={() => app.togglePanel("tree")}
      title="Show file tree"
      aria-label="Show file tree"
      class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center text-fg-3"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
    </button>
    {#if snapshot}
      <div class="mono text-[10px] text-muted [writing-mode:vertical-rl] mt-2">
        {files.length} files · {snapshot.reviewed_count}/{snapshot.total_count}
      </div>
    {/if}
  </div>
{:else}
<div class="w-64 border-r border-hairline bg-surface flex flex-col overflow-hidden transition-[width] duration-200">
  <!-- Search header -->
  <div class="flex items-center gap-2 px-3 py-2 border-b border-hairline shrink-0">
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2" class="shrink-0"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
    <input
      class="flex-1 bg-transparent text-sm text-fg-2 placeholder:text-muted outline-none min-w-0"
      placeholder="Filter files…"
      value={snapshot?.filter ?? ""}
      oninput={(e) => {
        const v = (e.target as HTMLInputElement).value;
        if (v) app.cmd("set_filter", { query: v });
        else app.cmd("clear_filter");
      }}
    />
    <span class="kbd">/</span>
  </div>

  <!-- Summary header -->
  {#if snapshot}
    <div class="flex items-center gap-3 px-3 py-1.5 border-b border-hairline text-[10px] mono text-muted shrink-0">
      <span>{files.length} files</span>
      {#if ai && ai.high > 0}
        <span class="flex items-center gap-1 text-risk-high"><span class="w-1.5 h-1.5 rounded-full bg-risk-high"></span>{ai.high}</span>
      {/if}
      {#if ai && ai.med > 0}
        <span class="flex items-center gap-1 text-risk-med"><span class="w-1.5 h-1.5 rounded-full bg-risk-med"></span>{ai.med}</span>
      {/if}
      {#if ai && ai.low > 0}
        <span class="flex items-center gap-1 text-risk-low"><span class="w-1.5 h-1.5 rounded-full bg-risk-low"></span>{ai.low}</span>
      {/if}
      {#if ai && (ai.comments > 0 || ai.questions > 0)}<span class="text-ink-400">·</span>{/if}
      {#if ai && ai.comments > 0}
        <span class="flex items-center gap-1 text-comment">
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
          {ai.comments}
        </span>
      {/if}
      {#if ai && ai.questions > 0}
        <span class="flex items-center gap-1 text-question">
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01"/></svg>
          {ai.questions}
        </span>
      {/if}
    </div>
  {/if}

  <!-- Tree -->
  <div class="flex-1 overflow-y-auto py-1.5 text-[13px]">
    {#if !snapshot}
      <div class="flex items-center justify-center h-full text-muted text-sm">Loading…</div>
    {:else if tree.length === 0}
      <div class="flex items-center justify-center h-full text-muted text-sm">No files</div>
    {:else}
      {#each tree as node (node.fullPath)}
        {#if node.kind === "folder"}
          <div
            class="flex items-center gap-1.5 pr-3 py-[3px] text-fg-3 cursor-default"
            style="padding-left: {indentPx(node.depth)};"
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
            <span class="truncate">{node.name}</span>
          </div>
        {:else if node.file}
          {@const file = node.file}
          {@const selected = (viewportPath ?? selectedFile?.path) === file.path}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            role="button"
            tabindex="0"
            class="flex items-center gap-1.5 pr-3 py-[3px] cursor-pointer relative {selected ? 'bg-hover' : 'hover:bg-card'}"
            style="padding-left: {indentPx(node.depth)};"
            onclick={() => jumpToFile(file.path, files.indexOf(file))}
            onkeydown={(e) => e.key === "Enter" && jumpToFile(file.path, files.indexOf(file))}
          >
            {#if selected}
              <span class="absolute left-0 top-0 bottom-0 w-[3px] bg-accent"></span>
            {/if}

            <span
              class="w-1.5 h-1.5 rounded-full shrink-0 {!file.reviewed && file.risk === 'high' ? 'bg-risk-high' : !file.reviewed && file.risk === 'med' ? 'bg-risk-med' : !file.reviewed && file.risk === 'low' ? 'bg-risk-low' : 'bg-transparent'}"
            ></span>

            <span class="truncate flex-1 {file.reviewed ? 'text-muted line-through' : selected ? 'text-fg' : 'text-fg-2'}">{node.name}</span>

            {#if file.reviewed}
              <span class="text-[10px] text-muted shrink-0">✓ reviewed</span>
            {:else}
              {#if file.finding_count > 0}
                <span class="text-[10px] mono shrink-0" style="color: {findingColor(file.risk)};">{file.finding_count}</span>
              {/if}
              {#if file.comment_count > 0}
                <span class="flex items-center gap-0.5 text-[10px] mono text-comment shrink-0">
                  <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>{file.comment_count}
                </span>
              {/if}
              {#if file.question_count > 0}
                <span class="flex items-center gap-0.5 text-[10px] mono text-question shrink-0">
                  <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01"/></svg>{file.question_count}
                </span>
              {/if}
              {#if file.additions > 0}
                <span class="text-[11px] mono text-add-fg shrink-0">+{file.additions}</span>
              {/if}
              {#if file.deletions > 0}
                <span class="text-[11px] mono text-del-fg shrink-0">−{file.deletions}</span>
              {/if}
            {/if}
          </div>
        {/if}
      {/each}
    {/if}
  </div>

  {#if snapshot}
    <ScopeSelector
      mode={snapshot.mode}
      total_count={snapshot.total_count}
      reviewed_count={snapshot.reviewed_count}
    />
  {/if}
</div>
{/if}
