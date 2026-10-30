<script lang="ts">
  import type { Vote } from "$lib/types/arena";
  import { VOTE_META } from "$lib/arena/arenaView";

  interface Props {
    vote?: Vote;
    /** Overrides the default "Round n: Keep"-style hover text. */
    title?: string;
  }

  const { vote, title }: Props = $props();
</script>

{#if vote}
  <span class="vote v-{vote}" title={title ?? VOTE_META[vote].label}>{VOTE_META[vote].glyph}</span>
{:else}
  <span class="vote v-none" title={title ?? "No vote"}>—</span>
{/if}

<style>
  .vote {
    display: inline-grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 5px;
    font-size: 11px;
    font-weight: 600;
    flex: none;
  }
  .v-propose {
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
  }
  .v-keep {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 14%, transparent);
  }
  .v-drop {
    color: var(--color-error);
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
  }
  .v-merge {
    color: var(--color-periwinkle);
    background: color-mix(in srgb, var(--color-periwinkle) 14%, transparent);
  }
  .v-escalate {
    color: var(--color-error);
    background: color-mix(in srgb, var(--color-error) 18%, transparent);
  }
  .v-lower,
  .v-flag {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 14%, transparent);
  }
  .v-abstain,
  .v-none {
    color: var(--color-muted);
  }
</style>
