import type { Meta, StoryObj } from "@storybook/svelte";
import ArenaLauncher from "$lib/components/arena/ArenaLauncher.svelte";

const meta = {
  title: "Arena/Launcher",
  component: ArenaLauncher,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ArenaLauncher>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Open: Story = {
  args: { open: true, onClose: () => {} },
};
