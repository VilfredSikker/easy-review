<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * A collapsible row inside a card. Collapsed, the header carries a one-line
   * teaser of the body, so a reader can scan every row without opening any.
   */
  interface Props {
    children: Snippet;
    label: string;
    /** Shown in place of the body while collapsed, truncated to the row. */
    preview?: string;
    /** A count after the label, e.g. how many rows the body holds. */
    badge?: number | string | null;
    open?: boolean;
  }

  let {
    children,
    label,
    preview = "",
    badge = null,
    open = $bindable(false),
  }: Props = $props();
</script>

<div class="min-w-0 border-t border-hairline first:border-t-0">
  <button
    type="button"
    class="flex w-full min-w-0 items-center gap-1.5 py-1.5 text-left text-muted transition-colors hover:text-fg-2"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <svg
      width="8"
      height="8"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2.5"
      class="shrink-0 transition-transform"
      class:rotate-90={open}
      aria-hidden="true"
    >
      <path d="M9 6l6 6-6 6" />
    </svg>
    <span class="shrink-0 text-[10px] uppercase tracking-wider">{label}</span>
    {#if badge != null}
      <span class="shrink-0 text-[10px] tabular-nums text-fg-3">{badge}</span>
    {/if}
    {#if !open && preview}
      <!-- A glance aid for sighted readers; in the button's name it would
           read the whole first line after the label. -->
      <span
        class="min-w-0 flex-1 truncate text-[11px] normal-case tracking-normal text-fg-3"
        aria-hidden="true"
      >
        {preview}
      </span>
    {/if}
  </button>
  {#if open}
    <div class="min-w-0 pb-2 pl-3.5">
      {@render children()}
    </div>
  {/if}
</div>
