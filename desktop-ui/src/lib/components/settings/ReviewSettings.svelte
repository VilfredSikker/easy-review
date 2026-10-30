<script lang="ts">
  import type { ConfigFieldValue, ConfigHubField } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import FieldRows from "./FieldRows.svelte";
  import RiskRulesSection from "./RiskRulesSection.svelte";
  import { fieldMap, pickFields } from "./settingsModel";
  import { COMMAND_DESCRIPTIONS, COMMAND_KEYS } from "./settingsSearch";
  import type { RiskFileRow, RiskRuleRow, RiskTier } from "./riskRules";

  interface Props {
    fields: ConfigHubField[];
    warnings: Record<string, string | null>;
    riskRepo: string;
    riskRules: RiskRuleRow[];
    riskDefault: RiskTier;
    riskFiles: RiskFileRow[];
    onpatch: (key: string, value: ConfigFieldValue) => Promise<void>;
    onriskedit: (edit: string) => Promise<RiskRuleRow[] | null>;
    onpropose: () => void;
  }

  const { fields, warnings, riskRepo, riskRules, riskDefault, riskFiles, onpatch, onriskedit, onpropose }: Props =
    $props();

  const commandFields = $derived(pickFields(fieldMap(fields), COMMAND_KEYS));
</script>

{#if commandFields.length > 0}
  <SettingsSection title="Review commands">
    <FieldRows fields={commandFields} {warnings} {onpatch} descriptions={COMMAND_DESCRIPTIONS} />
  </SettingsSection>
{/if}

<RiskRulesSection
  repo={riskRepo}
  rules={riskRules}
  defaultTier={riskDefault}
  files={riskFiles}
  onedit={onriskedit}
  {onpropose}
/>
