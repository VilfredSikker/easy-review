import type { Meta, StoryObj } from "@storybook/svelte";
import BranchContextBarHarness from "./BranchContextBarHarness.svelte";
import { richSnapshot } from "./fixtures";
import type { TabSummary } from "$lib/types";

const meta = {
  title: "Layout/BranchContextBar",
  component: BranchContextBarHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "rail" } },
} satisfies Meta<typeof BranchContextBarHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WorkingBranch: Story = {
  args: {
    snapshot: richSnapshot,
  },
};

export const RemotePrTab: Story = {
  args: {
    snapshot: {
      ...richSnapshot,
      branch: "dependabot/cargo/foo",
      base: "main",
      pr: null,
      github: {
        owner: "reshape",
        repo: "easy-review",
        number: 154,
        url: "https://github.com/reshape/easy-review/pull/154",
        state: "OPEN",
        is_draft: false,
        title: "Bump dependencies",
        body: "",
        author: "dependabot",
        head_ref: "dependabot/cargo/foo",
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
      },
      tabs: [
        {
          idx: 0,
          label: "octocat#154",
          kind: "remote_pr",
          branch: null,
          pr_number: 154,
          repo_root: "",
          is_active: true,
          change_token: "",
        } satisfies TabSummary,
      ],
      active_tab: 0,
    },
  },
};
