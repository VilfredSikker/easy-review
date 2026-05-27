<script lang="ts">
  const swatches = [
    ["bg-app", "var(--arena-bg-app)"],
    ["bg-0", "var(--arena-bg-0)"],
    ["bg-1", "var(--arena-bg-1)"],
    ["bg-2", "var(--arena-bg-2)"],
    ["orange", "var(--arena-orange)"],
    ["periwinkle", "var(--arena-periwinkle)"],
    ["ok", "var(--arena-ok)"],
    ["warn", "var(--arena-warn)"],
    ["err", "var(--arena-err)"],
    ["info", "var(--arena-info)"],
  ];
</script>

<div class="bg-[var(--arena-bg-app)] p-6">
  <h1 class="mb-4 text-[14px] font-semibold text-[var(--arena-fg)]">Arena tokens (§B9)</h1>
  <div class="grid grid-cols-2 gap-3 sm:grid-cols-5">
    {#each swatches as [name, color]}
      <div class="rounded-lg border border-[var(--arena-border)] p-2">
        <div class="mb-2 h-12 rounded" style="background:{color}"></div>
        <p class="mono text-[10px] text-[var(--arena-fg-muted)]">{name}</p>
      </div>
    {/each}
  </div>
</div>
