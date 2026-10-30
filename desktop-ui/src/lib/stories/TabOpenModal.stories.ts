import type { Meta, StoryObj } from "@storybook/svelte";
import TabOpenModal from "$lib/components/TabOpenModal.svelte";

const meta = {
  title: "Layout/TabOpenModal",
  component: TabOpenModal,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof TabOpenModal>;

export default meta;
type Story = StoryObj<typeof meta>;

const noop = () => {
  /* storybook stub */
};

const COMMENT_ICON = "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z";

export const FromComment: Story = {
  args: {
    request: {
      key: "pr:p1#1756",
      title: "Reorder db migrations to unblock deployment",
      ref: "discovery #1756",
      context: "Comment on your PR · 9 min ago",
      iconPath: COMMENT_ICON,
      tone: "comment",
      quote: "These four lines do the whole check. Could they move into a small function with a docstring?",
    },
    tabCount: 4,
    currentTabLabel: "#1761 mbs/dev-284-send-reminders",
    onChoose: noop,
    onClose: noop,
  },
};

export const NoQuote: Story = {
  args: {
    request: {
      key: "pr:p1#1752",
      title: "Plate maps: well picker keyboard support",
      ref: "discovery #1752",
      context: "Review requested · 3 h ago",
      iconPath: "M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
      tone: "accent",
      quote: null,
    },
    tabCount: 1,
    currentTabLabel: null,
    onChoose: noop,
    onClose: noop,
  },
};
