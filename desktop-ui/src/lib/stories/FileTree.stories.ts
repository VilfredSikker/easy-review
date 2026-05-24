import type { Meta, StoryObj } from "@storybook/svelte";
import FileTreeHarness from "./FileTreeHarness.svelte";
import { classicTreeSnapshot, multiFolderSnapshot } from "./fixtures";

const meta = {
  title: "Components/FileTree",
  component: FileTreeHarness,
  parameters: { layout: "centered" },
} satisfies Meta<typeof FileTreeHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Classic layout: status SVGs, › folders, sparkle findings, pipe summary header. */
export const ClassicFileTree: Story = {
  args: { snapshot: classicTreeSnapshot },
};

export const MultiFolder: Story = {
  args: { snapshot: multiFolderSnapshot },
};
