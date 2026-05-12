<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import ScopeSelector from "./ScopeSelector.svelte";

  const snapshot = $derived(app.snapshot);
  const files = $derived(snapshot?.files ?? []);
  const ai = $derived(snapshot?.ai);
</script>

<div class="w-64 border-r border-ink-500/40 bg-ink-850 flex flex-col overflow-hidden">
  <!-- Search header -->
  <div class="flex items-center gap-2 px-3 py-2 border-b border-ink-500/40 shrink-0">
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-ink-400 shrink-0"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
    <input
      class="flex-1 bg-transparent text-[13px] text-ink-100 placeholder:text-ink-500 outline-none min-w-0"
      placeholder="Filter files…"
      value={snapshot?.filter ?? ""}
      oninput={(e) => {
        const v = (e.target as HTMLInputElement).value;
        if (v) app.cmd("set_filter", { query: v });
        else app.cmd("clear_filter");
      }}
    />
    <span class="kbd text-[10px] text-ink-500 border border-ink-500/40 rounded px-1">/</span>
  </div>

  <!-- Summary header -->
  {#if snapshot}
    <div class="flex items-center gap-2 px-3 py-1.5 border-b border-ink-500/40 text-[10px] font-mono text-ink-400 shrink-0">
      <span>{files.length} files</span>
      {#if ai && ai.high > 0}
        <span class="text-risk-high">{ai.high}H</span>
      {/if}
      {#if ai && ai.med > 0}
        <span class="text-risk-med">{ai.med}M</span>
      {/if}
      {#if ai && ai.low > 0}
        <span class="text-risk-low">{ai.low}L</span>
      {/if}
      {#if ai && ai.comments > 0}
        <span class="flex items-center gap-0.5 text-comment">
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
          {ai.comments}
        </span>
      {/if}
      {#if ai && ai.questions > 0}
        <span class="flex items-center gap-0.5 text-question">
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01"/></svg>
          {ai.questions}
        </span>
      {/if}
    </div>
  {/if}

  <!-- File list -->
  <div class="flex-1 overflow-y-auto">
    {#if !snapshot}
      <div class="flex items-center justify-center h-full text-ink-300 text-sm">Loading…</div>
    {:else}
      {#each files as file, i}
        {@const selected = i === snapshot.selected_file}
        <div
          role="button"
          tabindex="0"
          class="pl-4 pr-3 py-[3px] flex items-center gap-1.5 cursor-pointer relative hover:bg-ink-800 {selected ? 'bg-ink-700' : ''}"
          onclick={() => app.cmd("select_file", { idx: i })}
          onkeydown={(e) => e.key === "Enter" && app.cmd("select_file", { idx: i })}
        >
          {#if selected}
            <span class="absolute left-0 top-0 bottom-0 w-[3px] bg-accent"></span>
          {/if}

          {#if !file.reviewed}
            <span
              class="w-1.5 h-1.5 rounded-full shrink-0 {file.risk === 'high' ? 'bg-risk-high' : file.risk === 'med' ? 'bg-risk-med' : file.risk === 'low' ? 'bg-risk-low' : 'bg-transparent'}"
            ></span>
          {/if}

          <span class="truncate flex-1 text-[13px] {file.reviewed ? 'text-ink-300 line-through' : 'text-ink-100'}">{file.path}</span>

          {#if file.reviewed}
            <span class="text-[10px] font-mono text-ink-400 shrink-0">✓ reviewed</span>
          {:else}
            {#if file.finding_count > 0}
              <span class="text-[10px] font-mono text-ink-400 shrink-0">{file.finding_count}</span>
            {/if}
            {#if file.comment_count > 0}
              <span class="text-[10px] font-mono text-comment shrink-0">
                <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="inline"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>{file.comment_count}
              </span>
            {/if}
            {#if file.question_count > 0}
              <span class="text-[10px] font-mono text-question shrink-0">
                <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="inline"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01"/></svg>{file.question_count}
              </span>
            {/if}
            {#if file.additions > 0}
              <span class="text-[11px] font-mono text-add-fg shrink-0">+{file.additions}</span>
            {/if}
            {#if file.deletions > 0}
              <span class="text-[11px] font-mono text-del-fg shrink-0">-{file.deletions}</span>
            {/if}
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <!-- Scope selector -->
  {#if snapshot}
    <ScopeSelector
      mode={snapshot.mode}
      total_count={snapshot.total_count}
      reviewed_count={snapshot.reviewed_count}
    />
  {/if}
</div>
