import type { Meta, StoryObj } from "@storybook/svelte";
import MainLayout from "./MainLayout.svelte";
import {
  richSnapshot,
  emptySnapshot,
  multiFolderSnapshot,
  remoteOnlyProjectSnapshot,
  prDraft,
  commitsRich,
  tabsWorkingActive,
} from "./fixtures";

const pinnedMock = [
  { id: "p1", title: "DEV-5008 Show experiment params", age: "5d" },
];

const projectsMock = [
  {
    name: "discovery-platform",
    branches: [
      { name: "show-experiment-params", age: "45m", state: "active" as const },
      { name: "fix-forward-button", age: "1d", state: "fork" as const },
      { name: "main", age: "", state: "branch" as const },
    ],
    badge: 3,
  },
  { name: "design-system", branches: [] },
  { name: "ink-booking", branches: [], badge: 2 },
];

const meta = {
  title: "Pages/MainLayout",
  component: MainLayout,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof MainLayout>;

export default meta;
type Story = StoryObj<typeof meta>;

/**
 * Mock 01-main recreation — the canonical "feature-complete" view.
 * Uses pinned + multi-project sidebar fixtures.
 */
export const Full: Story = {
  args: {
    snapshot: richSnapshot,
    pinned: pinnedMock,
    projects: projectsMock,
    titlebarSubtitle: "discovery-platform",
  },
};

/**
 * AI Review state — findings interleaved with hunks.
 */
export const AiReviewWithFindings: Story = {
  args: {
    snapshot: { ...richSnapshot, pr: null, commits: [] },
    pinned: pinnedMock,
    projects: projectsMock,
    titlebarSubtitle: "discovery-platform",
  },
};

/**
 * GitHub sync state — working tab with PR metadata + commit scroller visible.
 * ScopeSelector must show Unstaged/Staged and Commits despite snapshot.pr being set.
 */
export const GitHubSyncState: Story = {
  args: {
    snapshot: {
      ...richSnapshot,
      pr: prDraft,
      commits: commitsRich,
      tabs: tabsWorkingActive,
      active_tab: 0,
    },
    pinned: pinnedMock,
    projects: projectsMock,
    titlebarSubtitle: "discovery-platform",
  },
};

/**
 * Multi-folder tree — files spread across packages/, apps/, infra/.
 */
export const MultiFolder: Story = {
  args: {
    snapshot: multiFolderSnapshot,
    pinned: pinnedMock,
    projects: projectsMock,
    titlebarSubtitle: "discovery-platform",
  },
};

/**
 * Multi-project sidebar — Pinned + 3 projects + 1 nested worktree.
 */
export const MultiProject: Story = {
  args: {
    snapshot: richSnapshot,
    pinned: [
      { id: "p1", title: "DEV-5008 Show experiment params", age: "5d" },
      { id: "p2", title: "DEV-5142 Calendar widget", age: "2d" },
    ],
    projects: [
      {
        name: "discovery-platform",
        branches: [
          { name: "show-experiment-params", age: "45m", state: "active" as const },
          { name: "fix-forward-button", age: "1d", state: "fork" as const },
          { name: "main", age: "", state: "branch" as const },
        ],
        badge: 3,
      },
      { name: "design-system", branches: [] },
      { name: "ink-booking", branches: [], badge: 2 },
      { name: "marketing-site", branches: [] },
    ],
    titlebarSubtitle: "discovery-platform",
  },
};

export const RemoteOnlyProjectRecent: Story = {
  args: {
    snapshot: remoteOnlyProjectSnapshot,
    titlebarSubtitle: "owner/repo",
  },
};

/** Sparse snapshot — no AI data, no PR. */
export const SparseData: Story = {
  args: {
    snapshot: {
      ...emptySnapshot,
      branch: "main",
      base: "origin/main",
      worktrees: [{ path: "/Users/me/repo", branch: "main", is_current: true, is_pr: false, pr_number: null, is_merged: false }],
    },
  },
};
