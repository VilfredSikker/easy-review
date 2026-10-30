import type { Meta, StoryObj } from "@storybook/svelte";
import ViewBarHarness from "./ViewBarHarness.svelte";
import { localBranchBannerSnapshot, richSnapshot, staleSnapshot } from "./fixtures";
import type { AppSnapshot, GithubStatusSnapshot } from "$lib/types";

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

/** GitHub refuses an approval from the author, so Approve is disabled. */
export const OwnPr: Story = {
  args: { snapshot: withPr({ ...richSnapshot, mode: "pr" }, { is_authored_by_me: true }) },
};

/** A branch with no PR: PR Diff is disabled and there is nothing to approve. */
export const NoPr: Story = {
  args: { snapshot: { ...richSnapshot, mode: "branch", detected_pr_number: null, github: null } },
};
