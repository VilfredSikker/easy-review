import type { Meta, StoryObj } from "@storybook/svelte";
import FileRisksCard from "$lib/components/FileRisksCard.svelte";
import { aiFileRisksOnly } from "./fixtures";

const meta = {
  title: "RightPanel/FileRisksCard",
  component: FileRisksCard,
  parameters: { layout: "padded", backgrounds: { default: "rail" } },
} satisfies Meta<typeof FileRisksCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Ordered: Story = { args: { risks: aiFileRisksOnly.file_risks } };
