import type { Meta, StoryObj } from "@storybook/svelte";
import TriageCard from "$lib/components/TriageCard.svelte";
import {
  triageBeforeReach,
  triageBroadGuarded,
  triageExpert,
  triageLowSignal,
  triageSkip,
} from "./fixtures";

const meta = {
  title: "RightPanel/TriageCard",
  component: TriageCard,
  parameters: { layout: "padded", backgrounds: { default: "rail" } },
} satisfies Meta<typeof TriageCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const BroadGuarded: Story = { args: { triage: triageBroadGuarded } };
export const Stale: Story = { args: { triage: { ...triageBroadGuarded, fresh: false } } };
export const ExpertVerdict: Story = { args: { triage: triageExpert } };
export const SkipVerdict: Story = { args: { triage: triageSkip } };
export const BeforeReach: Story = { args: { triage: triageBeforeReach } };
/** Info risk, an unevidenced guard (no pill, no guard line) and a reach with no detail (no row). */
export const LowSignal: Story = { args: { triage: triageLowSignal } };
