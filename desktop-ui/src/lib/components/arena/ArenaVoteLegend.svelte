<script lang="ts">
  import { VOTE_META, VOTE_ORDER } from "$lib/arena/arenaView";
  import ArenaVoteCell from "$lib/components/arena/ArenaVoteCell.svelte";

  interface Props {
    class?: string;
  }

  const { class: className = "" }: Props = $props();
</script>

<div class="legend {className}" role="list" aria-label="Vote legend">
  {#each VOTE_ORDER as vote (vote)}
    <span role="listitem"><ArenaVoteCell {vote} />{VOTE_META[vote].label}</span>
  {/each}
</div>

<style>
  .legend {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 11px;
    color: var(--color-muted);
  }
  .legend > span {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
</style>
