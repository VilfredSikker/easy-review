<script lang="ts">
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import { narrativeHtml } from "$lib/tourModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "pillar-header" }>;
    reviewed: number;
    total: number;
    /** Tour-level freshness (ADR 0025): a stale tour stays readable, flagged. */
    fresh: boolean;
    /** Drawn diff paths; narrative references resolve only against these. */
    paths: readonly string[];
    onjump: (path: string) => void;
    onreviewpillar: (pillarId: string, allReviewed: boolean) => void;
  }
  const { row, reviewed, total, fresh, paths, onjump, onreviewpillar }: Props = $props();

  const allReviewed = $derived(total > 0 && reviewed >= total);
  const html = $derived(narrativeHtml(row.narrativeMarkdown || "", paths));

  function onProseClick(e: MouseEvent) {
    const ref = (e.target as HTMLElement | null)?.closest?.<HTMLElement>("[data-goto]");
    const path = ref?.dataset.goto;
    if (!path) return;
    e.preventDefault();
    onjump(path);
  }
</script>

<section class="pillar-row" class:first={row.index === 1} data-row-identity={row.identity}>
  <div class="pillar-head">
    <div class="min-w-0 flex-1">
      <div class="eyebrow">
        Pillar {row.index} of {row.total}
        {#if !fresh}
          <span class="stalechip" title="The diff changed since this tour was generated">stale</span>
        {/if}
      </div>
      <h2>{row.title}</h2>
    </div>
    <button
      type="button"
      class="rev"
      role="checkbox"
      aria-checked={allReviewed}
      title={allReviewed ? "Clear reviewed on every file in this pillar" : "Mark every file in this pillar reviewed"}
      onclick={() => onreviewpillar(row.pillarId, allReviewed)}
    >
      <span class="cb" aria-hidden="true">
        <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.5">
          <polyline points="20 6 9 17 4 12" />
        </svg>
      </span>
      <span class="lbl">Pillar reviewed</span>
    </button>
  </div>
  {#if html}
    <!-- The links are anchors, so they keep native keyboard activation; the
         click handler only routes them to the file instead of navigating. -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="prose" class:dim={!fresh} onclick={onProseClick}>
      <!-- eslint-disable-next-line svelte/no-at-html-tags -- narrativeHtml escapes every text run through renderInline before linking diff paths. -->
      {@html html}
    </div>
  {/if}
</section>

<style>
  .pillar-row {
    display: flow-root;
    padding: 32px 16px 8px;
    border-top: 1px solid var(--color-hairline);
    background: var(--color-bg);
    font-family: var(--font-sans);
  }
  /* The sticky file-header band already sits above the first row. */
  .pillar-row.first {
    padding-top: 12px;
    border-top: 0;
  }
  .pillar-head {
    display: flex;
    align-items: flex-start;
    gap: 16px;
    margin-bottom: 10px;
  }
  .eyebrow {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--color-fg-3);
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .stalechip {
    height: 16px;
    display: inline-flex;
    align-items: center;
    padding: 0 6px;
    border-radius: 4px;
    font-size: 10px;
    color: var(--color-risk-med);
    background: color-mix(in srgb, var(--color-risk-med) 14%, transparent);
  }
  h2 {
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.01em;
    margin: 2px 0 0;
    color: var(--color-fg);
    text-wrap: balance;
  }
  .rev {
    margin-top: 14px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--color-fg-2);
    white-space: nowrap;
  }
  .rev:hover {
    background: var(--color-hover);
    color: var(--color-fg);
  }
  .rev[aria-checked="true"] {
    color: var(--color-add-fg);
  }
  .cb {
    width: 14px;
    height: 14px;
    border-radius: 4px;
    border: 1.5px solid var(--color-border);
    display: inline-grid;
    place-items: center;
    background: var(--color-surface);
    color: transparent;
  }
  .rev[aria-checked="true"] .cb {
    background: var(--color-add-fg);
    border-color: var(--color-add-fg);
    color: var(--color-ground);
  }
  .prose {
    max-width: 68ch;
    color: var(--color-fg);
    font-size: 14px;
    line-height: 1.6;
    margin-bottom: 8px;
    overflow-wrap: anywhere;
  }
  .prose.dim {
    opacity: 0.72;
  }
  .prose :global(p),
  .prose :global(ul),
  .prose :global(ol),
  .prose :global(pre) {
    margin: 0 0 12px;
  }
  .prose :global(ul),
  .prose :global(ol) {
    padding-left: 1.2rem;
  }
  .prose :global(ul) {
    list-style: disc;
  }
  .prose :global(ol) {
    list-style: decimal;
  }
  .prose :global(code) {
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--color-hover);
    border: 1px solid var(--color-hairline);
    border-radius: 4px;
    padding: 0 4px;
  }
  .prose :global(pre) {
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--color-surface);
    border: 1px solid var(--color-hairline);
    border-radius: 8px;
    padding: 10px 12px;
    overflow-x: auto;
  }
  .prose :global(pre code) {
    white-space: pre;
    background: none;
    border: 0;
    padding: 0;
  }
  .prose :global(a) {
    text-decoration: underline;
  }
  .prose :global(a.ref) {
    color: var(--color-fg);
    text-decoration: underline dotted color-mix(in srgb, var(--color-fg) 40%, transparent);
    text-underline-offset: 3px;
  }
  .prose :global(a.ref:hover) {
    color: var(--color-accent);
  }
  @media (max-width: 880px) {
    .pillar-row {
      padding-inline: 12px;
    }
  }
  @media (max-width: 520px) {
    .lbl {
      display: none;
    }
  }
</style>
