<script lang="ts">
  import ArenaStatsStrip from "$lib/components/arena/ArenaStatsStrip.svelte";
  import { MOCK_ARENA_SNAPSHOT } from "$lib/arena/mockRun";
  import type { ArenaRunSnapshot } from "$lib/types/arena";

  const multi: ArenaRunSnapshot = {
    ...MOCK_ARENA_SNAPSHOT,
    panel: { rounds: 2, round1_confirmed: 2, round2_confirmed: 3, quorum_changed: true },
  };
  const single: ArenaRunSnapshot = {
    ...MOCK_ARENA_SNAPSHOT,
    panel: { rounds: 1, round1_confirmed: 3, quorum_changed: false },
  };
</script>

<div class="states">
  <section>
    <div class="caption">Multi-round panel · the measurement line</div>
    <ArenaStatsStrip snapshot={multi} effort={null} />
  </section>
  <section>
    <div class="caption">Single-round panel · no line</div>
    <ArenaStatsStrip snapshot={single} effort={null} />
  </section>
</div>

<style>
  .states {
    display: grid;
    gap: 18px;
  }
  section {
    border: 1px solid var(--color-hairline);
    border-radius: 10px;
    overflow: hidden;
  }
  .caption {
    padding: 8px 20px;
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: 600;
    color: var(--color-muted);
    border-bottom: 1px solid var(--color-hairline);
    background: var(--color-surface);
  }
</style>
