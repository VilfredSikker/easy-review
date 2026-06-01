import type { Meta, StoryObj } from "@storybook/svelte";
import ArenaVoteLegend from "$lib/components/arena/ArenaVoteLegend.svelte";
import ArenaFinalTruth from "$lib/components/arena/ArenaFinalTruth.svelte";
import ArenaSwatch from "./ArenaSwatch.svelte";
import { MOCK_MODEL_ARENA_SNAPSHOT } from "$lib/arena/mockRun";

const meta = {
  title: "Arena/Components",
  parameters: { layout: "padded" },
} satisfies Meta;

export default meta;

export const VoteLegend: StoryObj = {
  render: () => ({ Component: ArenaVoteLegend }),
};

export const FinalTruth: StoryObj = {
  render: () =>
    ({
      Component: ArenaFinalTruth,
      props: {
        snapshot: MOCK_MODEL_ARENA_SNAPSHOT,
        selectedId: MOCK_MODEL_ARENA_SNAPSHOT.run.findings[0]?.id ?? null,
        onSelect: () => {},
      },
    }) as never,
};

export const Tokens: StoryObj = {
  render: () => ({ Component: ArenaSwatch }),
  parameters: { layout: "fullscreen" },
};
