import type { Meta, StoryObj } from "@storybook/svelte";
import BranchContextBarHarness from "./BranchContextBarHarness.svelte";
import { richSnapshot } from "./fixtures";
import type { TabSummary } from "$lib/types";

const meta = {
  title: "Layout/BranchContextBar",
  component: BranchContextBarHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "rail" } },
} satisfies Meta<typeof BranchContextBarHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WorkingBranch: Story = {
  args: {
    snapshot: richSnapshot,
  },
};

export const RemotePrTab: Story = {
  args: {
    snapshot: {
      ...richSnapshot,
      branch: "dependabot/cargo/foo",
      base: "main",
      tabs: [
        {
          idx: 0,
          label: "octocat#154",
          kind: "remote_pr",
          branch: null,
          pr_number: 154,
          repo_root: "",
          is_active: true,
        } satisfies TabSummary,
      ],
      active_tab: 0,
    },
  },
};
