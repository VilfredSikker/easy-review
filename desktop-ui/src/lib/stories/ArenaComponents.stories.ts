import type { Meta, StoryObj } from "@storybook/svelte";
import ArenaVoteLegend from "$lib/components/arena/ArenaVoteLegend.svelte";
import ArenaFinalTruth from "$lib/components/arena/ArenaFinalTruth.svelte";
import ArenaFindingDetail from "$lib/components/arena/ArenaFindingDetail.svelte";
import ArenaStatsStrip from "$lib/components/arena/ArenaStatsStrip.svelte";
import ArenaSwatch from "./ArenaSwatch.svelte";
import { MOCK_ARENA_SNAPSHOT, MOCK_MODEL_ARENA_SNAPSHOT } from "$lib/arena/mockRun";

const meta = {
  title: "Arena/Components",
  parameters: { layout: "padded" },
} satisfies Meta;

export default meta;

export const VoteLegend: StoryObj = {
  render: () => ({ Component: ArenaVoteLegend }),
};

export const StatsStrip: StoryObj = {
  render: () =>
    ({
      Component: ArenaStatsStrip,
      props: { snapshot: MOCK_ARENA_SNAPSHOT, effort: "High" },
    }) as never,
};

export const FindingDetail: StoryObj = {
  render: () =>
    ({
      Component: ArenaFindingDetail,
      props: {
        snapshot: MOCK_ARENA_SNAPSHOT,
        findingId: MOCK_ARENA_SNAPSHOT.run.findings[4]?.id ?? null,
        onToggleExcluded: () => {},
        onKeepAnyway: () => {},
        onOpenInDiff: () => {},
      },
    }) as never,
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
