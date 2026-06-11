<script lang="ts">
  interface Props {
    checked: boolean;
    label: string;
    description?: string;
    disabled?: boolean;
    onchange: (checked: boolean) => void;
  }

  const { checked, label, description = "", disabled = false, onchange }: Props = $props();
</script>

<label class="flex items-center justify-between gap-4 py-3 group {disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}">
  <span class="min-w-0">
    <span class="block text-sm text-fg">{label}</span>
    {#if description}
      <span class="block text-xs text-muted mt-0.5">{description}</span>
    {/if}
  </span>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    disabled={disabled}
    class="relative shrink-0 w-9 h-5 rounded-full border transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-accent/40 {checked
      ? 'bg-accent border-accent'
      : 'bg-ink-700 border-border group-hover:border-ink-400'} disabled:cursor-not-allowed"
    onclick={() => {
      if (!disabled) onchange(!checked);
    }}
  >
    <span
      class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full shadow transition-all duration-150 {checked
        ? 'translate-x-4 bg-white'
        : 'bg-ink-200 group-hover:bg-ink-100'}"
    ></span>
  </button>
</label>
