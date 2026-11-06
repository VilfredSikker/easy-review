<script lang="ts">
  import type { Snippet } from "svelte";
  import { overviewCollapse } from "$lib/stores/overviewCollapse.svelte";

  interface Props {
    /** Storage key; stable across releases or a reader's folds are lost. */
    name: string;
    title: string;
    defaultOpen?: boolean;
    /** The section draws a Card: line the chevron up with its heading, and
     *  keep the card's outline while folded so the column keeps its rhythm. */
    card?: boolean;
    id?: string;
    class?: string;
    children: Snippet;
  }

  const { name, title, defaultOpen = true, card = false, id, class: extra = "", children }: Props = $props();
  const open = $derived(overviewCollapse.isOpen(name, defaultOpen));
  const toggle = () => overviewCollapse.toggle(name, defaultOpen);
</script>

<!-- The section's own heading stays where it was; folding swaps the body for
     a one-line stand-in with the same label, and the chevron sits in the
     gutter so no component needs a second header. -->
<section {id} class="relative {extra}" data-testid="overview-section-{name}" data-open={open}>
  <button
    type="button"
    class="absolute -left-4 grid h-4 w-3.5 place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg
      {card ? 'top-[17px]' : 'top-0'}"
    aria-expanded={open}
    aria-label="{open ? 'Collapse' : 'Expand'} {title}"
    title="{open ? 'Collapse' : 'Expand'} {title}"
    onclick={toggle}
  >
    <svg width="9" height="9" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" class="transition-transform {open ? 'rotate-90' : ''}" aria-hidden="true"><path d="M3.5 2l3 3-3 3" /></svg>
  </button>
  {#if open}
    {@render children()}
  {:else}
    <button
      type="button"
      class="flex w-full items-center text-left text-[11px] font-medium uppercase tracking-[.06em] text-fg-3 hover:text-fg-2
        {card ? 'rounded-xl border border-hairline bg-card px-4 py-[15px]' : ''}"
      onclick={toggle}
    >{title}</button>
  {/if}
</section>
