import type { Meta, StoryObj } from "@storybook/svelte";
import ArenaRunningPanel from "$lib/components/arena/ArenaRunningPanel.svelte";
import { MOCK_MODEL_ARENA_SNAPSHOT, makeRunningSnapshot } from "$lib/arena/mockRun";

const meta = {
  title: "Arena/Running",
  component: ArenaRunningPanel,
  parameters: { layout: "centered" },
} satisfies Meta<typeof ArenaRunningPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

const base = {
  open: true,
  minimized: false,
  config: {
    reviewers: MOCK_MODEL_ARENA_SNAPSHOT.run.config.reviewers,
    rounds: 3,
    scope: "branch" as const,
  },
  startedAt: Date.now() - 45_000,
  onMinimize: () => {},
  onRestore: () => {},
  onCancel: () => {},
  onComplete: () => {},
};

export const Round1: Story = {
  args: {
    ...base,
    snapshot: makeRunningSnapshot(1),
    progress: { round: 1, total_rounds: 3, thinking: ["claude-sonnet"], done: [] },
  },
};

export const Round3CrossCheck: Story = {
  args: {
    ...base,
    snapshot: makeRunningSnapshot(3),
    progress: { round: 3, total_rounds: 3, thinking: [], done: ["claude-sonnet", "openai-gpt"] },
  },
};

export const Arbiter: Story = {
  args: {
    ...base,
    snapshot: makeRunningSnapshot(3),
    progress: { round: 3, total_rounds: 3, phase: "arbiter", thinking: ["arbiter"], done: [] },
  },
};
