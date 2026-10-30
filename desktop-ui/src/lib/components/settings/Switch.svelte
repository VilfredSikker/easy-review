<script lang="ts">
  interface Props {
    checked: boolean;
    /** Accessible name; the visible label sits in the row beside it. */
    label: string;
    disabled?: boolean;
    onchange: (checked: boolean) => void;
  }

  const { checked, label, disabled = false, onchange }: Props = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  class="inline-flex items-center shrink-0 rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-accent/40 disabled:opacity-50 disabled:cursor-not-allowed"
  onclick={() => {
    if (!disabled) onchange(!checked);
  }}
>
  <span
    class="relative w-[26px] h-4 rounded-lg transition-colors duration-150 {checked ? 'bg-accent' : 'bg-ink-600'}"
    aria-hidden="true"
  >
    <span
      class="absolute top-0.5 left-0.5 w-3 h-3 rounded-full bg-card shadow-sm transition-transform duration-150 {checked
        ? 'translate-x-2.5'
        : ''}"
    ></span>
  </span>
</button>
