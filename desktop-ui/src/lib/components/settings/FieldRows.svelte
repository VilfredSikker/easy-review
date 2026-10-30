<script lang="ts">
  import type { ConfigFieldValue } from "$lib/types";
  import Toggle from "./Toggle.svelte";
  import OptionGroup from "./OptionGroup.svelte";
  import SettingsTextField from "./SettingsTextField.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import SettingsButton from "./SettingsButton.svelte";
  import { INPUT_CLASS, type ConfigValueField } from "./settingsModel";

  interface Props {
    fields: ConfigValueField[];
    warnings?: Record<string, string | null>;
    /** Per-key description overrides, where the page words a row differently
     *  from the engine's catalog. */
    descriptions?: Record<string, string>;
    onpatch: (key: string, value: ConfigFieldValue) => Promise<void>;
  }

  const { fields, warnings = {}, descriptions = {}, onpatch }: Props = $props();

  let addPattern = $state("");

  function fieldKey(field: ConfigValueField): string {
    return field.kind === "listEntry" ? `${field.key}-${field.index}` : field.key;
  }

  async function addEntry(key: string) {
    const pattern = addPattern.trim();
    if (!pattern) return;
    await onpatch(key, pattern);
    addPattern = "";
  }
</script>

{#each fields as field (fieldKey(field))}
  {#if field.kind === "bool"}
    <Toggle
      label={field.label}
      description={descriptions[field.key] ?? field.description}
      checked={field.value}
      onchange={(v) => void onpatch(field.key, v)}
    />
  {:else if field.kind === "cycle"}
    <OptionGroup
      label={field.label}
      description={descriptions[field.key] ?? field.description}
      options={field.options}
      value={field.value}
      onchange={(v) => void onpatch(field.key, v)}
    />
  {:else if field.kind === "text"}
    <SettingsTextField
      label={field.label}
      description={descriptions[field.key] ?? field.description}
      placeholder={field.placeholder}
      value={field.value}
      strict={field.strict}
      warning={warnings[field.key] ?? null}
      oncommit={(v) => void onpatch(field.key, v)}
    />
  {:else if field.kind === "listEntry"}
    <SettingsRow label={field.label} monoLabel>
      <button
        type="button"
        class="w-[22px] h-[22px] grid place-items-center rounded-[5px] text-[15px] leading-none text-fg-3 hover:bg-hover hover:text-risk-high"
        aria-label="Remove {field.label}"
        title="Remove"
        onclick={() => void onpatch(field.key.replace(/\.\d+$/, ".remove"), field.index)}
      >×</button>
    </SettingsRow>
  {:else if field.kind === "listAdd"}
    <SettingsRow always>
      <form
        class="flex flex-1 gap-2 min-w-0"
        onsubmit={(e) => {
          e.preventDefault();
          void addEntry(field.key);
        }}
      >
        <input
          type="text"
          class={INPUT_CLASS}
          aria-label={field.label}
          placeholder="Glob pattern, e.g. .work/**"
          bind:value={addPattern}
          onkeydown={(e) => e.stopPropagation()}
        />
        <SettingsButton type="submit" disabled={!addPattern.trim()}>Add</SettingsButton>
      </form>
    </SettingsRow>
  {/if}
{/each}
