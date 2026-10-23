import type { Meta, StoryObj } from "@storybook/svelte";
import ReviewModelGroupsEditor from "$lib/components/settings/ReviewModelGroupsEditor.svelte";
import type { AiProviderInfo, ReviewModelGroups } from "$lib/types";

const model = (id: string, label: string) => ({
  id, label, is_selected: false, effort_levels: [],
});
const providers: AiProviderInfo[] = [
  {
    id: "claude", label: "Claude", is_selected: true,
    models: [model("haiku-5.5", "Haiku 5.5"), model("sonnet-5.5", "Sonnet 5.5"), model("opus-5.5", "Opus 5.5")],
  },
  {
    id: "codex", label: "Codex", is_selected: false,
    models: [model("gpt-6-luna", "GPT-6 Luna"), model("gpt-6-sol", "GPT-6 Sol"), model("gpt-6-astra", "GPT-6 Astra")],
  },
  { id: "cursor", label: "Cursor", is_selected: false, models: [model("auto", "Auto")] },
];
const reviewModels: ReviewModelGroups = {
  fast: [{ provider_id: "claude", model_id: "haiku-5.5" }, { provider_id: "codex", model_id: "gpt-6-luna" }],
  standard: [{ provider_id: "claude", model_id: "sonnet-5.5" }, { provider_id: "codex", model_id: "gpt-6-sol" }],
  advanced: [{ provider_id: "claude", model_id: "opus-5.5" }, { provider_id: "codex", model_id: "gpt-6-astra" }],
};

const meta = {
  title: "Settings/ReviewModelGroups",
  component: ReviewModelGroupsEditor,
  parameters: { layout: "padded", backgrounds: { default: "rail" } },
  args: { providers, reviewModels, onUpdated: () => {}, onError: () => {} },
} satisfies Meta<typeof ReviewModelGroupsEditor>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Defaults: Story = {};
