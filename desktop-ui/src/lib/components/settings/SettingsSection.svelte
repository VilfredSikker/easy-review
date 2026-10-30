<script lang="ts">
  import type { Snippet } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { matchesQuery } from "./settingsModel";
  import { provideSectionSearch, useSettingsQuery } from "./settingsContext";

  interface Props {
    title: string;
    /** Right-aligned beside the heading: a count, a scope or a hint. */
    aside?: Snippet;
    children: Snippet;
    id?: string;
    /** Text the section renders outside any row (a table, a card grid). A
     *  match here, like one on the title, shows every row. Rows match on their
     *  own text and need nothing here. */
    keywords?: string[];
    /** Render the body without the card, for a section that lays out cards of its own. */
    bare?: boolean;
    danger?: boolean;
  }

  const { title, aside, children, id, keywords = [], bare = false, danger = false }: Props = $props();

  const query = useSettingsQuery();
  const searching = $derived(query().trim() !== "");
  const showAll = $derived(searching && matchesQuery(query(), title, ...keywords));
  const matchedRows = new SvelteSet<symbol>();
  provideSectionSearch({
    showAll: () => showAll,
    report: (row, matched) => {
      if (matched) matchedRows.add(row);
      else matchedRows.delete(row);
    },
    drop: (row) => matchedRows.delete(row),
  });

  const visible = $derived(!searching || showAll || matchedRows.size > 0);
</script>

<!-- Hidden rather than unmounted: the rows have to stay mounted to report a
     match that would bring the section back. -->
<section class="mb-[26px] scroll-mt-4" {id} aria-label={title} hidden={!visible}>
  <div class="flex items-baseline justify-between gap-2.5 mb-2">
    <h3 class="m-0 text-[13px] font-semibold {danger ? 'text-risk-high' : 'text-fg'}">{title}</h3>
    {#if aside}
      <span class="text-xs text-muted">{@render aside()}</span>
    {/if}
  </div>
  {#if bare}
    {@render children()}
  {:else}
    <div class="bg-card border border-line rounded-[10px] overflow-hidden">
      {@render children()}
    </div>
  {/if}
</section>
