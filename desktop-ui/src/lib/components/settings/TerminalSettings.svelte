<script lang="ts">
  import type { ConfigFieldValue, ConfigHubField } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import FieldRows from "./FieldRows.svelte";
  import { sectionsOf } from "./settingsModel";

  interface Props {
    fields: ConfigHubField[];
    warnings: Record<string, string | null>;
    onpatch: (key: string, value: ConfigFieldValue) => Promise<void>;
  }

  const { fields, warnings, onpatch }: Props = $props();

  // The engine titles these for the TUI's own config hub; the page uses the
  // prototype's sentence-case names and names each view by the key that opens it.
  const TITLES: Record<string, string> = { Views: "View modes", "Key Hints": "Key hints" };
  const VIEW_KEYS: Record<string, string> = {
    "features.view_branch": "Key 1",
    "features.view_unstaged": "Key 2",
    "features.view_staged": "Key 3",
    "features.view_history": "Key 4",
    "features.view_conflicts": "Key 5",
    "features.view_hidden": "Key 6",
  };

  const sections = $derived(sectionsOf(fields));
</script>

{#each sections as section, i (section.title ?? `untitled-${i}`)}
  {@const title = section.title ? (TITLES[section.title] ?? section.title) : "Other"}
  <SettingsSection {title} keywords={section.fields.flatMap((f) => [f.label, "description" in f ? f.description : ""])}>
    <FieldRows fields={section.fields} {warnings} {onpatch} descriptions={VIEW_KEYS} />
  </SettingsSection>
{/each}
