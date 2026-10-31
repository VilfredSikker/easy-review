import type { Meta, StoryObj } from "@storybook/svelte";
import OverviewView from "$lib/components/overview/OverviewView.svelte";
import type { AppSnapshot } from "$lib/types";
import {
  githubStatusOpen,
  localBranchEmptyBucketSnapshot,
  richSnapshot,
  tabsWorkingActive,
} from "./fixtures";

const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();

const prBody = `\`deploy-staging / deploy-db\` has been failing on main since #1745.

> [!WARNING]
> Anyone who ran the Hyperscale PR locally must run \`just reseed\` before pulling this.

- [x] Reproduced the staging error
- [ ] Deploy to staging and confirm deploy-db passes

Fixes https://linear.app/acme/issue/DEV-298 · Follow-up: #1758`;

const onPr: AppSnapshot = {
  ...richSnapshot,
  pr: null,
  tabs: [{ ...tabsWorkingActive[0], pr_number: githubStatusOpen.number }],
  github: {
    ...githubStatusOpen,
    body: prBody,
    created_at: minutesAgo(21),
    merge_state_status: "CLEAN",
    reviewers: [
      { reviewer: "martin", is_team: false, requested: false, state: "APPROVED" },
      { reviewer: "Copilot", is_team: false, requested: false, state: "COMMENTED" },
      { reviewer: "owner/backend", is_team: true, requested: true, state: null },
    ],
    recent_reviews: [
      { author: "martin", state: "APPROVED", body: "", submitted_at: minutesAgo(12) },
      { author: "copilot-pull-request-reviewer", state: "COMMENTED", body: "Two notes on the schema.", submitted_at: minutesAgo(20) },
    ],
    checks: [
      ...githubStatusOpen.checks,
      { name: "cleanup preview", status: "PENDING", conclusion: "", url: null },
    ],
  },
  ai: { ...richSnapshot.ai, review_created_at: minutesAgo(19) },
};

const meta = {
  title: "Overview/Page",
  component: OverviewView,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof OverviewView>;

export default meta;
type Story = StoryObj<typeof meta>;

/** An open PR: description, links, triage, findings and the aside's status, reviewers and checks. */
export const PullRequest: Story = { args: { snapshot: onPr } };

/** A local branch: no PR header or aside status; the meta line keeps branch and commits. */
export const LocalBranch: Story = { args: { snapshot: { ...richSnapshot, pr: null, github: null } } };

/** This view's bucket is empty while the other view's is not: one card replaces the AI sections. */
export const EmptyBucket: Story = { args: { snapshot: localBranchEmptyBucketSnapshot } };

/** Below 880px the aside drops under the main column. */
export const Narrow: Story = {
  args: { snapshot: onPr },
  parameters: { viewport: { defaultViewport: "mobile2" } },
};
