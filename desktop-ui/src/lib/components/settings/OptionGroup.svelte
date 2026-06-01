<script lang="ts">
  interface Props {
    label: string;
    description?: string;
    options: string[];
    value: string;
    onchange: (value: string) => void;
  }

  const { label, description = "", options, value, onchange }: Props = $props();

  function display(opt: string): string {
    const parts = opt.split("-");
    return parts.map((p) => p.charAt(0).toUpperCase() + p.slice(1)).join(" ");
  }
</script>

<div class="py-2">
  <div class="mb-1.5">
    <div class="text-sm text-fg">{label}</div>
    {#if description}
      <div class="text-xs text-muted mt-0.5">{description}</div>
    {/if}
  </div>
  <div class="flex flex-wrap gap-1">
    {#each options as opt (opt)}
      <button
        type="button"
        class="px-2.5 py-1 text-xs rounded-md border transition-colors {value === opt
          ? 'bg-accent text-black border-accent'
          : 'bg-surface text-fg-2 border-hairline hover:bg-hover'}"
        onclick={() => onchange(opt)}
      >
        {display(opt)}
      </button>
    {/each}
  </div>
</div>
