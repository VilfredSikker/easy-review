<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { ReviewerInfo } from "$lib/types";

  interface Props {
    selected: Set<string>;
    onSelectedChange: (selected: Set<string>) => void;
    /** Highlight row index for keyboard nav */
    highlightIdx?: number;
  }

  let { selected, onSelectedChange, highlightIdx = $bindable(0) }: Props = $props();

  let reviewers = $state<ReviewerInfo[]>([]);
  let loadError = $state<string | null>(null);
  let loading = $state(true);

  onMount(async () => {
    try {
      reviewers = await invoke<ReviewerInfo[]>("list_ai_reviewers");
      highlightIdx = 0;
    } catch (e) {
      loadError = String(e);
    } finally {
      loading = false;
    }
  });

  function toggle(kind: string) {
    const next = new Set(selected);
    if (next.has(kind)) next.delete(kind);
    else next.add(kind);
    onSelectedChange(next);
  }

  function selectAll() {
    onSelectedChange(new Set(reviewers.map((r) => r.kind)));
  }

  function clearAll() {
    onSelectedChange(new Set());
  }

  export function moveHighlight(delta: number) {
    if (reviewers.length === 0) return;
    highlightIdx = (highlightIdx + delta + reviewers.length) % reviewers.length;
  }

  export function toggleHighlighted() {
    const r = reviewers[highlightIdx];
    if (r) toggle(r.kind);
  }
</script>

{#if loading}
  <p class="px-4 py-6 text-sm text-ink-400">Loading reviewers…</p>
{:else if loadError}
  <p class="px-4 py-6 text-sm text-del-fg">{loadError}</p>
{:else}
  <div class="px-4 py-2 border-b border-ink-600 flex items-center gap-2 shrink-0">
    <button type="button" class="text-xs text-ink-300 hover:text-ink-100" onclick={selectAll}>Select all</button>
    <span class="text-ink-600">·</span>
    <button type="button" class="text-xs text-ink-300 hover:text-ink-100" onclick={clearAll}>Clear</button>
    <span class="ml-auto text-[10px] text-ink-400 font-mono">{selected.size} selected</span>
  </div>
  <ul class="flex-1 min-h-0 overflow-y-auto py-1" role="listbox" aria-multiselectable="true">
    {#each reviewers as r, i (r.kind)}
      <li>
        <button
          type="button"
          role="option"
          aria-selected={selected.has(r.kind)}
          class="w-full text-left px-4 py-2.5 flex items-start gap-3 hover:bg-ink-700/60 {i === highlightIdx ? 'bg-ink-700/80' : ''}"
          onclick={() => toggle(r.kind)}
        >
          <input
            type="checkbox"
            class="mt-0.5 shrink-0"
            checked={selected.has(r.kind)}
            tabindex={-1}
            onclick={(e) => e.stopPropagation()}
            onchange={() => toggle(r.kind)}
          />
          <span class="min-w-0">
            <span class="text-sm text-ink-100 font-medium">{r.label}</span>
            <span class="block text-[11px] text-ink-400 mt-0.5">{r.description}</span>
          </span>
        </button>
      </li>
    {/each}
  </ul>
{/if}
