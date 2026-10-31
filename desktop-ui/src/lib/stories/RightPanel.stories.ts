import type { Meta, StoryObj } from "@storybook/svelte";
import RightPanelHarness from "./RightPanelHarness.svelte";
import { aiEmpty, aiWithFindings, richSnapshot } from "./fixtures";

const meta = {
  title: "RightPanel/Panel",
  component: RightPanelHarness,
  parameters: { layout: "fullscreen" },
  argTypes: {
    tab: {
      control: { type: "select" },
      options: ["findings", "risks", "comments"],
    },
    collapsed: { control: { type: "boolean" } },
  },
} satisfies Meta<typeof RightPanelHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

const withFindings = { ...richSnapshot, ai: aiWithFindings };

export const Findings: Story = { args: { snapshot: withFindings, tab: "findings" } };

/** Risks reads the rules through `get_config_hub`, which Storybook has no
 *  backend for: the tab says so and shows every file as `ai`, rated by the
 *  review where it has a rating. */
export const Risks: Story = { args: { snapshot: withFindings, tab: "risks" } };

export const Comments: Story = { args: { snapshot: withFindings, tab: "comments" } };

export const EmptyView: Story = { args: { snapshot: { ...richSnapshot, ai: aiEmpty }, tab: "findings" } };

export const Collapsed: Story = { args: { snapshot: withFindings, collapsed: true } };
