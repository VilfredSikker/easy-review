<script lang="ts">
  import type { ConfigFieldValue, ConfigHubField } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import FieldRows from "./FieldRows.svelte";
  import { sectionsOf } from "./settingsModel";
  import { TERMINAL_TITLES, VIEW_KEY_DESCRIPTIONS } from "./settingsSearch";

  interface Props {
    fields: ConfigHubField[];
    warnings: Record<string, string | null>;
    onpatch: (key: string, value: ConfigFieldValue) => Promise<void>;
  }

  const { fields, warnings, onpatch }: Props = $props();

  const sections = $derived(sectionsOf(fields));
</script>

{#each sections as section, i (section.title ?? `untitled-${i}`)}
  {@const title = section.title ? (TERMINAL_TITLES[section.title] ?? section.title) : "Other"}
  <SettingsSection {title}>
    <FieldRows fields={section.fields} {warnings} {onpatch} descriptions={VIEW_KEY_DESCRIPTIONS} />
  </SettingsSection>
{/each}
