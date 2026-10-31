import type { Meta, StoryObj } from "@storybook/svelte";
import ViewBarHarness from "./ViewBarHarness.svelte";
import { localBranchBannerSnapshot, richSnapshot, staleSnapshot } from "./fixtures";
import type { AppSnapshot, GithubStatusSnapshot } from "$lib/types";
import type { PendingReview } from "$lib/reviewSubmit";

const meta = {
  title: "Shell/ViewBar",
  component: ViewBarHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "rail" } },
} satisfies Meta<typeof ViewBarHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

const openPr: GithubStatusSnapshot = {
  owner: "reshape",
  repo: "easy-review",
  number: 1756,
  url: "https://github.com/reshape/easy-review/pull/1756",
  state: "OPEN",
  is_draft: false,
  title: "Reorder db migrations to unblock deployment",
  body: "",
  author: "sul",
  head_ref: "sul/fix-migration-order",
  base_ref: "main",
  review_decision: null,
  mergeable: "MERGEABLE",
  labels: [],
  checks: [],
  comments_count: 0,
  reviews_count: 0,
  recent_comments: [],
  recent_reviews: [],
  last_updated: null,
  is_authored_by_me: false,
};

const withPr = (snap: AppSnapshot, over: Partial<GithubStatusSnapshot> = {}): AppSnapshot => ({
  ...snap,
  detected_pr_number: 1756,
  github: { ...openPr, ...over },
});

export const PrDiff: Story = {
  args: { snapshot: withPr({ ...richSnapshot, mode: "pr" }) },
};

/** The PR head moved: Stale pill in the bar and the amber banner under it. */
export const Stale: Story = {
  args: { snapshot: withPr(staleSnapshot) },
};

/** Local branch view of a local PR tab that diverges from the PR head. */
export const LocalBranch: Story = {
  args: { snapshot: withPr(localBranchBannerSnapshot) },
};

const pending: PendingReview = {
  pr_number: 1756,
  review_id: 9001,
  viewer: "vsd",
  review_request: { requested_by: "Sune", requested_at: new Date(Date.now() - 21 * 60_000).toISOString() },
  comments: [
    {
      id: "PRRC_1",
      database_id: 101,
      path: "db/migrations/20261027_add_index.sql",
      line: 14,
      start_line: 12,
      side: "RIGHT",
      body: "Does this need CONCURRENTLY? The table is large in prod.",
    },
    {
      id: "PRRC_2",
      database_id: 102,
      path: "db/schema.sql",
      line: 240,
      start_line: null,
      side: "RIGHT",
      body: "Version order looks right now — keep an eye on rows already applied.",
    },
  ],
};

const asReviewer = (snap: AppSnapshot): AppSnapshot => ({
  ...snap,
  github: snap.github && {
    ...snap.github,
    reviewers: [{ reviewer: "vsd", is_team: false, requested: true, state: null }],
  },
});

/** Two comments in the pending review: the Review button carries the count. */
export const PendingComments: Story = {
  args: { snapshot: withPr({ ...richSnapshot, mode: "pr" }), pending },
};

/** The Review popover: the request, pending comments, summary and decision. */
export const ReviewPopover: Story = {
  args: { snapshot: asReviewer(withPr({ ...richSnapshot, mode: "pr" })), pending, reviewOpen: true },
};

/** Nothing pending yet: the popover explains how comments collect. */
export const ReviewPopoverEmpty: Story = {
  args: {
    snapshot: withPr({ ...richSnapshot, mode: "pr" }),
    pending: { ...pending, review_id: null, comments: [], review_request: null },
    reviewOpen: true,
  },
};

/** GitHub refuses an approval from the author, so the popover offers Approve disabled. */
export const OwnPr: Story = {
  args: {
    snapshot: withPr({ ...richSnapshot, mode: "pr" }, { is_authored_by_me: true }),
    pending: { ...pending, comments: [], review_request: null },
    reviewOpen: true,
  },
};

/** A branch with no PR: PR Diff and Review are disabled with the reason. */
export const NoPr: Story = {
  args: { snapshot: { ...richSnapshot, mode: "branch", detected_pr_number: null, github: null } },
};
