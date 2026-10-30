<script lang="ts">
  import SettingsRow from "./SettingsRow.svelte";
  import { INPUT_CLASS } from "./settingsModel";

  interface Props {
    label: string;
    description?: string;
    placeholder?: string;
    value: string;
    strict?: boolean;
    warning?: string | null;
    oncommit: (value: string) => void;
  }

  let {
    label,
    description = "",
    placeholder = "",
    value,
    strict = false,
    warning = null,
    oncommit,
  }: Props = $props();

  // Editable draft; resets whenever the committed value changes.
  let draft = $derived(value);
</script>

<SettingsRow {label} {description}>
  <div class="flex flex-col items-stretch gap-1 min-w-[240px] flex-1">
    <input
      type="text"
      aria-label={label}
      class={INPUT_CLASS}
      {placeholder}
      bind:value={draft}
      onblur={() => {
        if (draft !== value) oncommit(draft);
      }}
      onkeydown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
      }}
    />
    {#if strict && warning}
      <p class="flex items-center gap-1.5 text-xs text-risk-med">
        <span aria-hidden="true">⚠</span>
        {warning}
      </p>
    {/if}
  </div>
</SettingsRow>
