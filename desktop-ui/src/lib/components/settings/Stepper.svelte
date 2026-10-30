<script lang="ts">
  import { stepOption } from "./settingsModel";

  interface Props {
    /** The engine's option list; the stepper never leaves it. */
    options: string[];
    value: string;
    label: string;
    onchange: (value: string) => void;
  }

  const { options, value, label, onchange }: Props = $props();

  const down = $derived(stepOption(options, value, -1));
  const up = $derived(stepOption(options, value, 1));
</script>

<div class="inline-flex items-center border border-border rounded-[7px] overflow-hidden" role="group" aria-label={label}>
  <button
    type="button"
    class="w-[26px] h-[26px] text-fg-3 hover:bg-hover hover:text-fg disabled:opacity-40 disabled:hover:bg-transparent"
    aria-label="Decrease {label}"
    disabled={down === null}
    onclick={() => down !== null && onchange(down)}
  >−</button>
  <span class="min-w-7 text-center font-mono text-xs text-fg" aria-live="polite">{value}</span>
  <button
    type="button"
    class="w-[26px] h-[26px] text-fg-3 hover:bg-hover hover:text-fg disabled:opacity-40 disabled:hover:bg-transparent"
    aria-label="Increase {label}"
    disabled={up === null}
    onclick={() => up !== null && onchange(up)}
  >+</button>
</div>
