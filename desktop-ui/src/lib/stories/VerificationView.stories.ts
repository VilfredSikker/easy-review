import type { Meta, StoryObj } from "@storybook/svelte";
import type { ArenaRunSnapshot } from "$lib/types/arena";
import { MOCK_ARENA_SNAPSHOT } from "$lib/arena/mockRun";
import VerificationView from "$lib/components/arena/VerificationView.svelte";
import ArenaHarness from "./ArenaHarness.svelte";

const flowSnapshot: ArenaRunSnapshot = {
  ...MOCK_ARENA_SNAPSHOT,
  run: {
    ...MOCK_ARENA_SNAPSHOT.run,
    config: { ...MOCK_ARENA_SNAPSHOT.run.config, run_kind: "review_flow" },
  },
  arbiter: { label: "Opus 5", provider_id: "anthropic", model_id: "opus-5" },
};

const meta = {
  title: "Arena/Verification results",
  parameters: { layout: "fullscreen" },
} satisfies Meta;

export default meta;

/** A Deep pass opened from the review flow: results wait for Accept. */
export const AwaitingAccept: StoryObj = {
  render: () =>
    ({
      Component: VerificationView,
      props: { snapshot: flowSnapshot, flowAcceptance: true, onClose: () => {} },
    }) as never,
};

/** The same pass reopened from history: read only, nothing to accept. */
export const HistoricalFlow: StoryObj = {
  render: () => ({ Component: ArenaHarness, props: { snapshot: flowSnapshot } }) as never,
};
