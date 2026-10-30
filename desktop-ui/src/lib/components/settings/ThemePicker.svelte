<script lang="ts">
  import { APP_THEMES } from "$lib/themes";
  import { optionLabel } from "./settingsModel";

  interface Props {
    /** The engine's theme names; a name themes.ts does not define still gets a
     *  card, drawn in the current theme's own colors. */
    options: string[];
    value: string;
    onchange: (value: string) => void;
  }

  const { options, value, onchange }: Props = $props();

  // Swatch colors come from the same token set the theme applies (ADR 0030),
  // so a card always previews what picking it will paint.
  const swatches = $derived(
    options.map((name) => {
      const theme = APP_THEMES.find((t) => t.name === name);
      return {
        name,
        canvas: theme?.bg ?? "var(--color-bg)",
        panel: theme?.panel ?? "var(--color-card)",
        chips: theme ? [theme.accent, theme.green, theme.red] : ["var(--color-accent)", "var(--color-success)", "var(--color-error)"],
      };
    }),
  );
</script>

<div class="grid grid-cols-2 min-[880px]:grid-cols-4 gap-2 px-3.5 pb-3.5" role="radiogroup" aria-label="Theme">
  {#each swatches as swatch (swatch.name)}
    {@const on = swatch.name === value}
    <button
      type="button"
      role="radio"
      aria-checked={on}
      class="grid gap-1.5 p-1.5 rounded-[9px] border text-left transition-colors {on
        ? 'border-accent ring-1 ring-accent'
        : 'border-line hover:border-border'}"
      onclick={() => {
        if (!on) onchange(swatch.name);
      }}
    >
      <span
        class="relative h-[46px] rounded-md flex items-end gap-1 p-1.5 overflow-hidden border border-border"
        style:background={swatch.canvas}
        aria-hidden="true"
      >
        <span class="absolute left-[30%] top-2 -right-1 -bottom-1 rounded-tl-[5px]" style:background={swatch.panel}></span>
        {#each swatch.chips as chip, i (i)}
          <i class="relative w-2.5 h-2.5 rounded-[3px]" style:background={chip}></i>
        {/each}
      </span>
      <span class="text-xs {on ? 'text-fg font-medium' : 'text-fg-3'}">{optionLabel(swatch.name)}</span>
    </button>
  {/each}
</div>
