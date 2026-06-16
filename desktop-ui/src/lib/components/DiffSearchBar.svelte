<script lang="ts">
  /**
   * Cmd+F lexical search bar for the diff view (PR #73). A dense single-row
   * overlay in the top-right of the diff viewport, positioned just below the
   * 40px sticky file-path header (`top-12`) so the two don't overlap. Typing drives the shared
   * reference-highlight store in "query" mode (substring + smart-case), so
   * matches render with the same inline highlight, ruler marks, and
   * jump-to-flash as identifier clicks. Enter/Shift+Enter and the arrow keys
   * navigate matches with wrap-around; Esc closes the bar and clears the
   * highlight.
   */
  import { onMount } from "svelte";
  import { refHighlight } from "$lib/stores/referenceHighlight.svelte";

  interface Props {
    /** Total individual match ranges (possibly capped). */
    total: number;
    /** True when the match list was truncated at the cap. */
    capped: boolean;
    /** Current match index (−1 = none yet). */
    activeIdx: number;
    onNavigate: (dir: 1 | -1) => void;
  }
  const { total, capped, activeIdx, onNavigate }: Props = $props();

  const DEBOUNCE_MS = 150;

  let inputEl: HTMLInputElement | null = $state(null);
  // Seed from the active identifier highlight (Cmd+F prefills it as the query).
  let value = $state(refHighlight.identifier ?? "");
  let lastStoreQuery = refHighlight.identifier ?? "";
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Re-sync the input when the store query changes from outside — clicking an
  // identifier in the diff while the bar is open routes through setQuery(),
  // and a Cmd+click (usages popover) re-targets the highlight directly.
  $effect(() => {
    const q = refHighlight.identifier ?? "";
    if (q === lastStoreQuery) return;
    lastStoreQuery = q;
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    value = q;
  });

  function flushQuery(): void {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    if ((refHighlight.identifier ?? "") !== value) {
      lastStoreQuery = value;
      refHighlight.setQuery(value);
    }
  }

  function onInput(e: Event): void {
    value = (e.currentTarget as HTMLInputElement).value;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      lastStoreQuery = value;
      refHighlight.setQuery(value);
    }, DEBOUNCE_MS);
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "Enter") {
      e.preventDefault();
      flushQuery();
      onNavigate(e.shiftKey ? -1 : 1);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      flushQuery();
      onNavigate(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      flushQuery();
      onNavigate(-1);
    } else if (e.key === "Escape") {
      e.preventDefault();
      refHighlight.closeSearch();
    }
  }

  // Focus + select whenever the bar is (re)opened: the epoch bumps on every
  // openSearch() call, so Cmd+F focuses the input even when the bar is
  // already mounted but focus has moved elsewhere. Runs on mount too (the
  // first epoch bump happens before the bar renders), covering all three
  // open paths: empty, identifier prefill, selection prefill.
  $effect(() => {
    void refHighlight.searchFocusEpoch;
    inputEl?.focus();
    inputEl?.select();
  });

  onMount(() => {
    return () => {
      if (debounceTimer) clearTimeout(debounceTimer);
    };
  });

  const counter = $derived.by(() => {
    if (value.length === 0) return null;
    if (total === 0) return "0 / 0";
    const current = activeIdx >= 0 ? activeIdx + 1 : 1;
    return `${current} / ${capped ? "5000+" : total}`;
  });
</script>

<div
  class="absolute top-12 right-4 z-40 flex items-center gap-2 bg-card border border-hairline rounded-md px-2 py-1 shadow-lg"
  data-diff-search-bar
>
  <input
    bind:this={inputEl}
    type="text"
    class="mono text-[13px] bg-transparent outline-none border-none w-52 text-fg placeholder:text-fg-3"
    placeholder="Search diff"
    spellcheck="false"
    autocomplete="off"
    {value}
    oninput={onInput}
    onkeydown={onKeydown}
    aria-label="Search in diff"
  />
  {#if counter !== null}
    <span class="mono text-[11px] text-fg-3 tabular-nums whitespace-nowrap shrink-0">{counter}</span>
  {/if}
  <button
    type="button"
    class="p-0.5 text-fg-3 hover:bg-hover rounded shrink-0 disabled:opacity-40"
    onclick={() => onNavigate(-1)}
    disabled={total === 0}
    title="Previous match (Shift+Enter)"
    aria-label="Previous match"
  >
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m18 15-6-6-6 6"/></svg>
  </button>
  <button
    type="button"
    class="p-0.5 text-fg-3 hover:bg-hover rounded shrink-0 disabled:opacity-40"
    onclick={() => onNavigate(1)}
    disabled={total === 0}
    title="Next match (Enter)"
    aria-label="Next match"
  >
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 9 6 6 6-6"/></svg>
  </button>
  <span class="kbd shrink-0">esc</span>
</div>
