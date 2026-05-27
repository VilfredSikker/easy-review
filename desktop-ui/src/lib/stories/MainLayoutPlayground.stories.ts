import type { Meta, StoryObj } from "@storybook/svelte";
import MainLayoutPlayground from "./MainLayoutPlayground.svelte";

const meta = {
  title: "Pages/MainLayout Playground",
  component: MainLayoutPlayground,
  parameters: { layout: "fullscreen" },
  argTypes: {
    leftRail: {
      control: { type: "select" },
      options: ["expanded", "collapsed", "hidden"],
      description: "Left sidebar state",
    },
    treeRail: {
      control: { type: "boolean" },
      description: "Show file tree panel",
    },
    rightRail: {
      control: { type: "select" },
      options: ["expanded", "collapsed", "hidden"],
      description: "Right panel state",
    },
    terminalOpen: {
      control: { type: "boolean" },
      description: "Show terminal drawer",
    },
    inboxOpen: {
      control: { type: "boolean" },
      description:
        "Seed inbox items (popover opens on click of the Inbox header in the sidebar)",
    },
    mockTerminal: {
      control: { type: "boolean" },
      description:
        "Use visual terminal stand-in (true by default — avoids Tauri invoke crash in Storybook)",
    },
  },
} satisfies Meta<typeof MainLayoutPlayground>;

export default meta;
type Story = StoryObj<typeof meta>;

/**
 * Interactive playground — use the Controls panel to toggle every panel,
 * the terminal drawer, and fire toasts via the bottom-left button bar.
 */
export const Playground: Story = {
  args: {
    leftRail: "expanded",
    treeRail: true,
    rightRail: "expanded",
    terminalOpen: false,
    inboxOpen: false,
    mockTerminal: true,
  },
};

/**
 * All panels open, terminal visible.
 */
export const AllPanelsOpen: Story = {
  args: {
    leftRail: "expanded",
    treeRail: true,
    rightRail: "expanded",
    terminalOpen: true,
    inboxOpen: false,
    mockTerminal: true,
  },
};

/**
 * Right rail collapsed to the narrow icon strip — shows CollapsedRightRail.
 */
export const RightRailCollapsed: Story = {
  args: {
    leftRail: "expanded",
    treeRail: true,
    rightRail: "collapsed",
    terminalOpen: false,
    inboxOpen: false,
    mockTerminal: true,
  },
};

/**
 * Terminal drawer open with the mock prompt visible.
 */
export const TerminalOpen: Story = {
  args: {
    leftRail: "expanded",
    treeRail: true,
    rightRail: "expanded",
    terminalOpen: true,
    inboxOpen: false,
    mockTerminal: true,
  },
};

/**
 * Minimal chrome — left sidebar hidden, file tree collapsed, right panel hidden.
 * Maximum diff viewport.
 */
export const MinimalChromeOnly: Story = {
  args: {
    leftRail: "hidden",
    treeRail: false,
    rightRail: "hidden",
    terminalOpen: false,
    inboxOpen: false,
    mockTerminal: true,
  },
};
