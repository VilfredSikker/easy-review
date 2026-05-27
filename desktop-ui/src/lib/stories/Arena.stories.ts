import type { Meta, StoryObj } from "@storybook/svelte";
import ArenaHarness from "./ArenaHarness.svelte";
import ArenaSwatch from "./ArenaSwatch.svelte";

const harnessMeta = {
  title: "Arena/Overlay",
  component: ArenaHarness,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ArenaHarness>;

export default harnessMeta;
type HarnessStory = StoryObj<typeof harnessMeta>;

export const MockRun: HarnessStory = {};

const swatchMeta = {
  title: "Arena/Tokens",
  component: ArenaSwatch,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ArenaSwatch>;

export const TokenSwatch: StoryObj<typeof swatchMeta> = {};
