import type { Meta, StoryObj } from "@storybook/svelte";
import TabStrip from "$lib/components/TabStrip.svelte";
import type { TabSummary } from "$lib/types";

function tab(overrides: Partial<TabSummary> & { idx: number; label: string }): TabSummary {
  return {
    kind: "working",
    branch: null,
    pr_number: null,
    repo_root: "/Users/me/code/repo",
    is_active: false,
    ...overrides,
  };
}

const meta = {
  title: "Layout/TabStrip",
  component: TabStrip,
  parameters: { layout: "fullscreen", backgrounds: { default: "rail" } },
} satisfies Meta<typeof TabStrip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const SingleTab: Story = {
  args: {
    tabs: [tab({ idx: 0, label: "easy-review", is_active: true })],
    active: 0,
    showToolbar: false,
  },
};

export const MultiTab: Story = {
  args: {
    showToolbar: false,
    tabs: [
      tab({ idx: 0, label: "easy-review", is_active: true }),
      tab({
        idx: 1,
        label: "feat/multi-tab",
        kind: "local_branch",
        branch: "feat/multi-tab",
      }),
      tab({
        idx: 2,
        label: "octocat#154",
        kind: "remote_pr",
        pr_number: 154,
        repo_root: "",
      }),
    ],
    active: 0,
  },
};

export const ManyTabs: Story = {
  args: {
    showToolbar: false,
    tabs: Array.from({ length: 12 }, (_, i) =>
      tab({
        idx: i,
        label:
          i % 3 === 0
            ? `easy-review`
            : i % 3 === 1
              ? `feat/branch-${i}`
              : `octocat#${100 + i}`,
        kind: i % 3 === 0 ? "working" : i % 3 === 1 ? "local_branch" : "remote_pr",
        branch: i % 3 === 1 ? `feat/branch-${i}` : null,
        pr_number: i % 3 === 2 ? 100 + i : null,
        is_active: i === 4,
      }),
    ),
    active: 4,
  },
};

// Marker: tabs in the strip support HTML5 drag-and-drop reorder. The drop
// triggers the `reorder_tabs` Tauri command, which mutates `App.tabs` and
// adjusts `active_tab` so the focused tab stays focused. There's no visual
// difference vs `MultiTab` here — try it in the live app.
export const Reorderable: Story = {
  name: "MultiTab (Reorderable)",
  args: {
    showToolbar: false,
    tabs: [
      tab({ idx: 0, label: "easy-review", is_active: true }),
      tab({
        idx: 1,
        label: "feat/multi-tab",
        kind: "local_branch",
        branch: "feat/multi-tab",
      }),
      tab({
        idx: 2,
        label: "octocat#154",
        kind: "remote_pr",
        pr_number: 154,
        repo_root: "",
      }),
    ],
    active: 0,
  },
};
