import type { Meta, StoryObj } from "@storybook/svelte";
import OverviewActivity from "$lib/components/overview/OverviewActivity.svelte";
import { botConversation } from "./botCommentFixtures";

const meta = {
  title: "Overview/Activity",
  component: OverviewActivity,
  parameters: { layout: "padded" },
  args: { github: botConversation, ai: null, loadImage: () => Promise.reject(new Error("offline")), onOpenRef: () => {} },
} satisfies Meta<typeof OverviewActivity>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Linear and Vercel bots plus a long human reply: HTML renders, nothing widens the column. */
export const BotComments: Story = {};

/** The column at its narrowest: tables and code scroll inside their card. */
export const BotCommentsNarrow: Story = {
  parameters: { viewport: { defaultViewport: "mobile2" } },
};
