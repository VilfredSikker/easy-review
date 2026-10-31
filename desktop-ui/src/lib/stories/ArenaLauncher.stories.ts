import type { Meta, StoryObj } from "@storybook/svelte";
import ArenaLauncherHarness from "./ArenaLauncherHarness.svelte";

const meta = {
  title: "Arena/Launcher",
  component: ArenaLauncherHarness,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ArenaLauncherHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Open: Story = {};
