<script lang="ts">
  import type { Snippet } from "svelte";
  import { matchesQuery } from "./settingsModel";
  import { useSectionSearch, useSettingsQuery } from "./settingsContext";

  interface Props {
    label?: string;
    description?: string;
    /** `col` stacks the control under the label; `tight` is the inbox matrix density. */
    layout?: "row" | "col" | "tight";
    monoLabel?: boolean;
    /** Controls the row carries that a reader might search for, such as a
     *  button's label. */
    keywords?: string[];
    /** Ignore the search: rows that are part of another row's control (an add form). */
    always?: boolean;
    labelSlot?: Snippet;
    children?: Snippet;
  }

  const {
    label = "",
    description = "",
    layout = "row",
    monoLabel = false,
    keywords = [],
    always = false,
    labelSlot,
    children,
  }: Props = $props();

  const query = useSettingsQuery();
  const section = useSectionSearch();
  const matched = $derived(!!(label || description || keywords.length) && matchesQuery(query(), label, description, ...keywords));
  const visible = $derived(always || !!section?.showAll() || matched);

  // An `always` row reports too: it renders its text whatever the query, so a
  // match on it is a match the section shows.
  const id = Symbol("settings-row");
  $effect(() => {
    if (!section) return;
    section.report(id, matched);
    return () => section.drop(id);
  });

  const hasLabel = $derived(!!(label || description || labelSlot));
  const pad = $derived(layout === "tight" ? "py-2" : "py-[11px]");
  const flow = $derived(
    layout === "col" ? "flex-col items-stretch" : "items-center justify-between flex-wrap",
  );
</script>

{#if visible}
  <div class="flex gap-x-4 gap-y-2 px-3.5 {pad} {flow} border-t border-line first:border-t-0">
    {#if hasLabel}
      <div class="min-w-0 {layout === 'col' ? '' : 'flex-[1_1_220px]'}">
        {#if labelSlot}
          {@render labelSlot()}
        {:else if label}
          <div class="text-[13px] font-medium text-fg {monoLabel ? 'font-mono text-xs' : ''}">{label}</div>
        {/if}
        {#if description}
          <div class="text-xs text-fg-3 mt-px leading-[1.45]">{description}</div>
        {/if}
      </div>
    {/if}
    {#if children}
      <div class="flex items-center gap-2 flex-wrap min-w-0 {layout === 'col' ? '' : 'justify-end'} {hasLabel ? '' : 'flex-1'}">
        {@render children()}
      </div>
    {/if}
  </div>
{/if}
