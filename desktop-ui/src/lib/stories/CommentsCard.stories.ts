import type { Meta, StoryObj } from "@storybook/svelte";
import CommentsCard from "$lib/components/CommentsCard.svelte";
import { aiWithFindings } from "./fixtures";

const meta = {
  title: "RightPanel/CommentsCard",
  component: CommentsCard,
  parameters: { layout: "padded", backgrounds: { default: "rail" } },
} satisfies Meta<typeof CommentsCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const GithubOnlyOutdated: Story = {
  args: {
    ai: {
      ...aiWithFindings,
      local_comment_count: 0,
      github_comment_count: 1,
      comments: 1,
      threads: [
        {
          id: "gh-thread-1",
          kind: "comment",
          file: "src/main.rs",
          line: 42,
          source: "github",
          synced: true,
          stale: true,
          resolved: false,
          root: {
            id: "gh-thread-1",
            author: "octocat",
            kind: "human",
            timestamp: new Date().toISOString(),
            body_markdown: "This GitHub comment is outdated.",
          },
          replies: [],
          promoted_to: null,
        },
      ],
    },
  },
};
