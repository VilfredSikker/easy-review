import type { Meta, StoryObj } from "@storybook/svelte";
import TabStrip from "$lib/components/TabStrip.svelte";
import type { TabSummary } from "$lib/types";
import { tabSeen } from "$lib/stores/tabSeen.svelte";

function tab(overrides: Partial<TabSummary> & { idx: number; label: string }): TabSummary {
  return {
    kind: "working",
    branch: null,
    pr_number: null,
    repo_root: "/Users/me/code/repo",
    is_active: false,
    change_token: "",
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

// "New changes" indicator: TabStrip shows a periwinkle dot on a tab when
// `tabSeen.hasUnseen(idx, change_token)` returns true. hasUnseen returns false
// for an idx that was never passed to markSeen, so the dot only lights up when
// there is a previously-recorded baseline that differs from the current token.
// Seeding strategy: call `tabSeen.markSeen(idx, "v0")` in beforeEach to
// establish the baseline, then give the tab `change_token: "v1"` so the tokens
// differ. Tabs that should NOT show the dot either (a) are active, (b) have
// change_token matching their seeded baseline, or (c) have an empty change_token
// with no seeded baseline.
export const WithNewChangesIndicator: Story = {
  beforeEach: () => {
    // Seed a baseline for tabs 1 and 2 so hasUnseen can detect a change.
    // Tab 3 gets no baseline (no dot). Tab 0 is active (TabStrip skips it).
    tabSeen.markSeen(1, "v0");
    tabSeen.markSeen(2, "v0");
  },
  args: {
    showToolbar: true,
    active: 0,
    tabs: [
      // Active tab — dot never shown on the active tab.
      tab({ idx: 0, label: "easy-review", is_active: true }),
      // Dot shows: local_branch tab seeded at "v0", now at "v1".
      tab({ idx: 1, label: "feat/new-ui", kind: "local_branch", branch: "feat/new-ui", change_token: "v1" }),
      // Dot shows: remote PR tab seeded at "v0", now at "v1".
      tab({ idx: 2, label: "octocat#512", kind: "remote_pr", pr_number: 512, repo_root: "", change_token: "v1" }),
      // No dot: working tab with change_token matching its baseline (same token).
      tab({ idx: 3, label: "feature-b", kind: "local_branch", branch: "feature-b", change_token: "" }),
    ],
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
