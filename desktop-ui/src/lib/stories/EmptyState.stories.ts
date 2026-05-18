import type { Meta, StoryObj } from "@storybook/svelte";
import EmptyState from "$lib/components/EmptyState.svelte";

const meta = {
  title: "Pages/EmptyState",
  component: EmptyState,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof EmptyState>;

export default meta;
type Story = StoryObj<typeof meta>;

export const FirstLaunch: Story = {};
