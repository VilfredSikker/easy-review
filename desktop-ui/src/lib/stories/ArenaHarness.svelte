<script lang="ts">
  import ArenaOverlay from "$lib/components/arena/ArenaOverlay.svelte";
  import type { ArenaLayoutMode } from "$lib/components/arena/ArenaOverlay.svelte";
  import { MOCK_ARENA_SNAPSHOT } from "$lib/arena/mockRun";

  let open = $state(true);
  let layoutMode = $state<ArenaLayoutMode>("bracket");
</script>

<div class="h-screen bg-[var(--arena-bg-app)] p-4">
  <p class="mb-3 text-[12px] text-[var(--arena-fg-muted)]">
    Arena overlay (mock snapshot) — toggle layout in header · Esc closes
  </p>
  <button
    type="button"
    class="mb-3 rounded-md bg-[var(--arena-orange)] px-3 py-1.5 text-[12px] font-semibold text-white"
    onclick={() => (open = !open)}
  >
    {open ? "Hide" : "Show"} arena
  </button>

  <ArenaOverlay
    {open}
    snapshot={MOCK_ARENA_SNAPSHOT}
    bind:layoutMode
    onClose={() => (open = false)}
    onNewRun={() => alert("arena_start — wired in S9")}
  />
</div>
