<script lang="ts">
  import { optionLabel } from "./settingsModel";

  interface Props {
    options: string[];
    value: string;
    ariaLabel: string;
    display?: (option: string) => string;
    disabled?: boolean;
    onchange: (value: string) => void;
  }

  const { options, value, ariaLabel, display = optionLabel, disabled = false, onchange }: Props = $props();
</script>

<div class="inline-flex flex-wrap max-w-full p-0.5 rounded-[7px] bg-hover" role="group" aria-label={ariaLabel}>
  {#each options as option (option)}
    <button
      type="button"
      aria-pressed={value === option}
      {disabled}
      class="px-[9px] py-0.5 rounded-[5px] text-xs font-medium transition-colors disabled:opacity-50 {value === option
        ? 'bg-card text-fg ring-1 ring-border'
        : 'text-fg-3 hover:text-fg'}"
      onclick={() => {
        if (value !== option) onchange(option);
      }}
    >
      {display(option)}
    </button>
  {/each}
</div>
