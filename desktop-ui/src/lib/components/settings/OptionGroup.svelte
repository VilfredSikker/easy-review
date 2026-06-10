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

<div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 py-3">
  <div class="min-w-0">
    <div class="text-sm text-fg">{label}</div>
    {#if description}
      <div class="text-xs text-muted mt-0.5">{description}</div>
    {/if}
  </div>
  <div class="inline-flex min-w-0 max-w-full p-0.5 gap-0.5 bg-ink-850 border border-hairline rounded-lg flex-wrap justify-end">
    {#each options as opt (opt)}
      <button
        type="button"
        aria-pressed={value === opt}
        class="px-2.5 py-1 text-xs rounded-md transition-colors {value === opt
          ? 'bg-accent-soft text-accent font-medium'
          : 'text-fg-3 hover:text-fg hover:bg-hover'}"
        onclick={() => onchange(opt)}
      >
        {display(opt)}
      </button>
    {/each}
  </div>
</div>
