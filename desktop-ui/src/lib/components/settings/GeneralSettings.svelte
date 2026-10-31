<script lang="ts">
  import type { ConfigFieldValue, ConfigHubField } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import FieldRows from "./FieldRows.svelte";
  import ThemePicker from "./ThemePicker.svelte";
  import InboxSettings from "./InboxSettings.svelte";
  import UninstallSection from "./UninstallSection.svelte";
  import { fieldMap, optionLabel, unclaimedSections } from "./settingsModel";
  import { GENERAL_ROWS, WATCHED_DESCRIPTIONS } from "./settingsSearch";

  interface Props {
    fields: ConfigHubField[];
    warnings: Record<string, string | null>;
    testBusy: boolean;
    focusUninstall: boolean;
    onuninstallfocused: () => void;
    onpatch: (key: string, value: ConfigFieldValue) => Promise<void>;
    ontest: () => void;
  }

  const { fields, warnings, testBusy, focusUninstall, onuninstallfocused, onpatch, ontest }: Props = $props();

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
  <SettingsSection title="Appearance">
    <SettingsRow {...GENERAL_ROWS.theme} keywords={theme.options.map(optionLabel)} layout="col" />
    <ThemePicker options={theme.options} value={theme.value} onchange={(v) => void onpatch(theme.key, v)} />
  </SettingsSection>
{/if}

{#if watched.length > 0}
  <SettingsSection title="Watched paths">
    <FieldRows fields={watched} {warnings} {onpatch} descriptions={WATCHED_DESCRIPTIONS} />
  </SettingsSection>
{/if}

<InboxSettings {fields} {testBusy} {onpatch} {ontest} />

{#each rest as section, i (section.title ?? `untitled-${i}`)}
  <SettingsSection title={section.title ?? "Other"}>
    <FieldRows fields={section.fields} {warnings} {onpatch} />
  </SettingsSection>
{/each}

<UninstallSection autoPreview={focusUninstall} onautopreviewed={onuninstallfocused} />
