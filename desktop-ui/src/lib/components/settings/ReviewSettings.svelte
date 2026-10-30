<script lang="ts">
  import type { ConfigFieldValue, ConfigHubField } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import FieldRows from "./FieldRows.svelte";
  import RiskRulesSection from "./RiskRulesSection.svelte";
  import { fieldMap, pickFields } from "./settingsModel";
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

  const COMMAND_KEYS = [
    "commands.summary",
    "commands.test",
    "commands.lint",
    "commands.typecheck",
    "commands.security",
    "summary.push_to_pr",
  ] as const;

  const commandFields = $derived(pickFields(fieldMap(fields), COMMAND_KEYS));
</script>

<SettingsSection title="Review commands" keywords={commandFields.flatMap((f) => [f.label])}>
  <FieldRows
    fields={commandFields}
    {warnings}
    {onpatch}
    descriptions={{
      "summary.push_to_pr": "Write the review summary into the PR description.",
    }}
  />
</SettingsSection>

<RiskRulesSection
  repo={riskRepo}
  rules={riskRules}
  defaultTier={riskDefault}
  files={riskFiles}
  onedit={onriskedit}
  {onpropose}
/>
