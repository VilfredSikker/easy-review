<script lang="ts">
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "lazy-stub" }>;
  }
  const { row }: Props = $props();
</script>

<!--
  Fills the file's full reserved height with a dim line-rhythm skeleton instead
  of a single label over an empty (black) area. The faint bars cadence on the
  24px diff line grid so a not-yet-loaded file reads as "content loading" rather
  than a black gap while `request_file_content` round-trips.
-->
<div class="lazy-skeleton" style="height:{row.height}px" data-row-identity={row.identity}>
  <span class="lazy-skeleton-label">Loading content…</span>
</div>

<style>
  .lazy-skeleton {
    position: relative;
    overflow: hidden;
    background-color: var(--color-ink-850);
  }
  /* Line-rhythm bars: a faint bar inset from a left gutter, repeated every 24px
     (matches LINE_HEIGHT in diffRenderModel.ts). */
  .lazy-skeleton::before {
    content: "";
    position: absolute;
    inset: 28px 16px 0 48px;
    background-image: repeating-linear-gradient(
      to bottom,
      transparent 0px,
      transparent 7px,
      var(--color-ink-700) 7px,
      var(--color-ink-700) 17px,
      transparent 17px,
      transparent 24px
    );
  }
  .lazy-skeleton-label {
    position: absolute;
    top: 6px;
    left: 16px;
    font-size: 0.75rem;
    color: var(--color-muted);
  }
</style>
