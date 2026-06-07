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

<label class="flex items-start justify-between gap-4 py-2 group {disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}">
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
    disabled={disabled}
    class="relative shrink-0 w-9 h-5 rounded-full border transition-colors {checked
      ? 'bg-accent border-accent'
      : 'bg-ink-700 border-hairline'} disabled:cursor-not-allowed"
    onclick={() => {
      if (!disabled) onchange(!checked);
    }}
  >
    <span
      class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform {checked
        ? 'translate-x-4'
        : ''}"
    ></span>
  </button>
</label>
