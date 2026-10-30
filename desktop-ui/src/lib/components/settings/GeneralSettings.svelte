<script lang="ts">
  import type { ConfigFieldValue, ConfigHubField } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import FieldRows from "./FieldRows.svelte";
  import ThemePicker from "./ThemePicker.svelte";
  import InboxSettings from "./InboxSettings.svelte";
  import UninstallSection from "./UninstallSection.svelte";
  import { fieldMap, unclaimedSections } from "./settingsModel";

  interface Props {
    fields: ConfigHubField[];
    warnings: Record<string, string | null>;
    testBusy: boolean;
    focusUninstall: boolean;
    onpatch: (key: string, value: ConfigFieldValue) => Promise<void>;
    ontest: () => void;
  }

  const { fields, warnings, testBusy, focusUninstall, onpatch, ontest }: Props = $props();

  const byKey = $derived(fieldMap(fields));
  const theme = $derived.by(() => {
    const field = byKey.get("display.theme");
    return field?.kind === "cycle" ? field : null;
  });
  const watched = $derived(
    fields.flatMap((f) => (f.kind !== "section" && f.key.startsWith("watched.") ? [f] : [])),
  );
  const rest = $derived(unclaimedSections(fields));
</script>

{#if theme}
  <SettingsSection title="Appearance" keywords={["Theme", ...theme.options]}>
    <SettingsRow label="Theme" description="One token set for the desktop app and the TUI." layout="col" always />
    <ThemePicker options={theme.options} value={theme.value} onchange={(v) => void onpatch(theme.key, v)} />
  </SettingsSection>
{/if}

<SettingsSection title="Watched paths" keywords={watched.map((f) => f.label)}>
  <FieldRows
    fields={watched}
    {warnings}
    {onpatch}
    descriptions={{
      "watched.diff_mode":
        "How to diff files under watched paths: by content, or against a snapshot taken when you open the tab.",
    }}
  />
</SettingsSection>

<InboxSettings {fields} {testBusy} {onpatch} {ontest} />

{#each rest as section, i (section.title ?? `untitled-${i}`)}
  <SettingsSection title={section.title ?? "Other"} keywords={section.fields.map((f) => f.label)}>
    <FieldRows fields={section.fields} {warnings} {onpatch} />
  </SettingsSection>
{/each}

<UninstallSection autoPreview={focusUninstall} />
