<script lang="ts">
  import type { Snippet } from "svelte";
  import { matchesQuery } from "./settingsModel";
  import { provideSectionMatched, useSettingsQuery } from "./settingsContext";

  interface Props {
    title: string;
    /** Right-aligned beside the heading: a count, a scope or a hint. */
    aside?: Snippet;
    children: Snippet;
    id?: string;
    /** The rows' searchable text. With a query that matches neither these nor
     *  the title the section hides; without them it always shows. */
    keywords?: string[];
    /** Render the body without the card, for a section that lays out cards of its own. */
    bare?: boolean;
    danger?: boolean;
  }

  const { title, aside, children, id, keywords, bare = false, danger = false }: Props = $props();

  const query = useSettingsQuery();
  const titleMatched = $derived(query().trim() !== "" && matchesQuery(query(), title));
  provideSectionMatched(() => titleMatched);

  const visible = $derived(!keywords || titleMatched || matchesQuery(query(), ...keywords));
</script>

{#if visible}
  <section class="mb-[26px] scroll-mt-4" {id} aria-label={title}>
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
{/if}
