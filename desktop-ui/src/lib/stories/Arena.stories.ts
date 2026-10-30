import type { Meta, StoryObj } from "@storybook/svelte";
import ArenaHarness from "./ArenaHarness.svelte";
import { MOCK_ARENA_SNAPSHOT, MOCK_MODEL_ARENA_SNAPSHOT } from "$lib/arena/mockRun";

const meta = {
  title: "Arena/Page",
  component: ArenaHarness,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ArenaHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Matrix: Story = {
  args: { layoutMode: "matrix", snapshot: MOCK_ARENA_SNAPSHOT },
};

export const MatrixTwoModels: Story = {
  args: { layoutMode: "matrix", snapshot: MOCK_MODEL_ARENA_SNAPSHOT },
};

export const Bracket: Story = {
  args: { layoutMode: "bracket", snapshot: MOCK_ARENA_SNAPSHOT },
};

export const Funnel: Story = {
  args: { layoutMode: "funnel", snapshot: MOCK_ARENA_SNAPSHOT },
};

export const Cancelled: Story = {
  args: {
    layoutMode: "matrix",
    snapshot: {
      ...MOCK_ARENA_SNAPSHOT,
      run: { ...MOCK_ARENA_SNAPSHOT.run, status: "cancelled", completed_at: undefined },
    },
  },
};
