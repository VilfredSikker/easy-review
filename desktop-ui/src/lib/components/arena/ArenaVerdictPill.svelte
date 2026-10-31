<script lang="ts">
  import type { Verdict } from "$lib/types/arena";
  import { VERDICT_LABEL, verdictKey } from "$lib/arena/arenaView";

  interface Props {
    verdict: Verdict;
  }

  const { verdict }: Props = $props();
  const key = $derived(verdictKey(verdict));
</script>

<span class="vpill vp-{key}">{VERDICT_LABEL[key]}</span>

<style>
  .vpill {
    display: inline-flex;
    height: 19px;
    align-items: center;
    padding: 0 7px;
    border-radius: 5px;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
  }
  .vp-kept {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 14%, transparent);
  }
  .vp-escalated {
    color: var(--color-error);
    background: color-mix(in srgb, var(--color-error) 14%, transparent);
  }
  .vp-merged {
    color: var(--color-periwinkle);
    background: color-mix(in srgb, var(--color-periwinkle) 14%, transparent);
  }
  .vp-dropped {
    color: var(--color-muted);
    background: var(--color-card);
    text-decoration: line-through;
  }
  .vp-pending {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
  }
</style>
