import type { Meta, StoryObj } from "@storybook/svelte";
import BranchCard from "$lib/components/BranchCard.svelte";
import type { GithubStatusSnapshot } from "$lib/types";
import { prDraft } from "./fixtures";

const meta = {
  title: "RightPanel/BranchCard",
  component: BranchCard,
  parameters: { layout: "padded", backgrounds: { default: "rail" } },
} satisfies Meta<typeof BranchCard>;

export default meta;
type Story = StoryObj<typeof meta>;

function baseGithub(overrides: Partial<GithubStatusSnapshot> = {}): GithubStatusSnapshot {
  return {
    owner: "reshape",
    repo: "easy-review",
    number: 42,
    url: "https://github.com/reshape/easy-review/pull/42",
    state: "OPEN",
    is_draft: false,
    title: "Wire SourcesCard to live GitHub data",
    body: "## Summary\n\nReplaces the placeholder cards with a real wiring to the live GitHub status snapshot. Pulls title, reviewers, mergeable state, and labels straight from `gh pr view`.\n\n## Test plan\n- [x] Open a PR-mode tab, confirm the card shows up\n- [x] Toggle draft mode and verify the pill flips\n- [x] Refresh button re-fetches",
    author: "vilfred",
    head_ref: "feat/sources-live",
    base_ref: "main",
    review_decision: "APPROVED",
    mergeable: "MERGEABLE",
    labels: ["frontend", "polish"],
    checks: [
      { name: "test", status: "COMPLETED", conclusion: "SUCCESS", url: "https://ci/1" },
      { name: "lint", status: "COMPLETED", conclusion: "SUCCESS", url: "https://ci/2" },
      { name: "build", status: "COMPLETED", conclusion: "SUCCESS", url: "https://ci/3" },
    ],
    comments_count: 3,
    reviews_count: 1,
    recent_comments: [],
    recent_reviews: [],
    last_updated: String(Math.floor(Date.now() / 1000)),
    ...overrides,
  };
}

export const WithPr: Story = {
  args: {
    branch: "show-experiment-params",
    base: "main",
    pr: prDraft,
    reviewed_count: 2,
    total_count: 5,
    additions: 1927,
    deletions: 10,
    checks_status: "success",
  },
};

export const NoPr: Story = {
  args: {
    branch: "feat/cleanup",
    base: "main",
    pr: null,
    reviewed_count: 0,
    total_count: 3,
    additions: 42,
    deletions: 7,
    checks_status: null,
  },
};

export const ChecksPending: Story = {
  args: {
    branch: "wip/refactor",
    base: "main",
    pr: { number: 99, title: "WIP refactor", state: "draft", base: "main", head: "wip/refactor" },
    reviewed_count: 1,
    total_count: 4,
    additions: 100,
    deletions: 50,
    checks_status: "pending",
  },
};

export const WithGitHub_AllGreen: Story = {
  args: {
    branch: "feat/sources-live",
    base: "main",
    pr: null,
    reviewed_count: 3,
    total_count: 5,
    additions: 1927,
    deletions: 10,
    checks_status: null,
    github: baseGithub(),
  },
};

export const WithGitHub_ChecksFailing: Story = {
  args: {
    branch: "feat/sources-live",
    base: "main",
    pr: null,
    reviewed_count: 1,
    total_count: 5,
    additions: 200,
    deletions: 40,
    checks_status: null,
    github: baseGithub({
      review_decision: "REVIEW_REQUIRED",
      mergeable: "CONFLICTING",
      checks: [
        { name: "test", status: "COMPLETED", conclusion: "SUCCESS", url: "https://ci/1" },
        { name: "lint", status: "COMPLETED", conclusion: "FAILURE", url: "https://ci/2" },
        { name: "build", status: "PENDING", conclusion: "", url: null },
      ],
    }),
  },
};
