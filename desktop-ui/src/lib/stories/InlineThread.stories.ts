import type { Meta, StoryObj } from "@storybook/svelte";
import InlineThread from "$lib/components/InlineThread.svelte";
import { commentThread, questionThread } from "./fixtures";

const meta = {
  title: "Diff/InlineThread",
  component: InlineThread,
  parameters: { layout: "padded", backgrounds: { default: "app" } },
} satisfies Meta<typeof InlineThread>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Comment: Story = { args: { thread: commentThread, hunk_idx: 0 } };
export const Question: Story = { args: { thread: questionThread, hunk_idx: 0 } };
export const Synced: Story = {
  args: { thread: { ...commentThread, synced: true, source: "github" }, hunk_idx: 0 },
};
export const Stale: Story = {
  args: { thread: { ...commentThread, stale: true }, hunk_idx: 0 },
};

/**
 * Question with the inline reply composer initially closed. Click "Reply" to
 * expand the textarea — Cmd+Enter submits, Esc cancels.
 */
export const WithInlineComposer: Story = {
  args: { thread: questionThread, hunk_idx: 0 },
};

/**
 * A question that has already been promoted to a GitHub comment. The
 * "Promote to comment" action is hidden and a "Promoted to #N" footer
 * is shown instead.
 */
export const AlreadyPromoted: Story = {
  args: {
    thread: { ...questionThread, promoted_to: "c-1738592100000-42" },
    hunk_idx: 0,
  },
};
