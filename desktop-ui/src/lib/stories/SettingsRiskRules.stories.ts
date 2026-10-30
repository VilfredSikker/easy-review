import type { Meta, StoryObj } from "@storybook/svelte";
import RiskRulesSection from "$lib/components/settings/RiskRulesSection.svelte";
import type { RiskRuleRow } from "$lib/components/settings/riskRules";

const meta = {
  title: "Settings/RiskRules",
  component: RiskRulesSection,
  parameters: { layout: "padded", backgrounds: { default: "app" } },
} satisfies Meta<typeof RiskRulesSection>;

export default meta;
type Story = StoryObj<typeof meta>;

const rules: RiskRuleRow[] = [
  { pattern: "packages/ris-db/db/**", tier: "high", readable: true, label: "Migrations", source: "project", fileCount: 3 },
  { pattern: "src/auth/**", tier: "high", readable: true, label: "Auth", source: "project", fileCount: 1 },
  { pattern: "src/**", tier: "ai", readable: true, label: "Application logic", source: "project", fileCount: 6 },
  { pattern: "*.md", tier: "low", readable: true, label: "Docs", source: "user", fileCount: 0 },
  { pattern: "*.lock", tier: "lw", readable: false, label: null, source: "user", fileCount: 0 },
];

const noop = () => {
  /* storybook stub */
};

export const Declared: Story = {
  args: {
    repo: "discovery",
    rules,
    defaultTier: "ai",
    files: [
      { path: "packages/ris-db/db/0042_plates.sql", tier: "high", rule: { key: "packages/ris-db/db/**", label: "Migrations", source: "project" } },
      { path: "src/plates/picker.ts", tier: "ai", rule: { key: "src/**", label: "Application logic", source: "project" } },
      { path: "README", tier: "ai", rule: null },
    ],
    onedit: async () => rules,
    onpropose: noop,
  },
};

export const Empty: Story = {
  args: { repo: "lab-agents", rules: [], defaultTier: "ai", files: [], onedit: async () => [], onpropose: noop },
};
