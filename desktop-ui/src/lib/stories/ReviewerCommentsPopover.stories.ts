import type { Meta, StoryObj } from "@storybook/svelte";
import ReviewerCommentsPopover from "$lib/components/overview/ReviewerCommentsPopover.svelte";
import type { ThreadSnapshot } from "$lib/types";

const at = "2026-10-30T10:00:00Z";

const threads: ThreadSnapshot[] = [
  {
    id: "t1",
    kind: "comment",
    file: "db/schema.sql",
    line: 24561,
    source: "github",
    synced: true,
    stale: false,
    resolved: false,
    root: { id: "c1", author: "martin", kind: "human", timestamp: at, body_markdown: "Databases that already applied the 20261027* versions keep those rows. Worth a note in the runbook?" },
    replies: [{ id: "c2", author: "sune", kind: "human", timestamp: at, body_markdown: "Added to the README." }],
    promoted_to: null,
  },
  {
    id: "t2",
    kind: "comment",
    file: "ci/check-migrations.sh",
    line: 12,
    line_end: 18,
    source: "github",
    synced: true,
    stale: false,
    resolved: false,
    root: { id: "c3", author: "sune", kind: "human", timestamp: at, body_markdown: "Strict mode here?" },
    replies: [{ id: "c4", author: "martin", kind: "human", timestamp: at, body_markdown: "Yes, --strict rejects the out-of-order case." }],
    promoted_to: null,
  },
];

const meta = {
  title: "Overview/ReviewerCommentsPopover",
  component: ReviewerCommentsPopover,
  parameters: { layout: "padded" },
  args: {
    reviewer: { reviewer: "martin", is_team: false, requested: false, state: "APPROVED" },
    displayName: "Martin Lyngaa Durhuus",
    reviews: [{ author: "martin", state: "APPROVED", body: "", submitted_at: at }],
    comments: [],
    threads,
    onJump: () => {},
    onShowInPanel: () => {},
  },
} satisfies Meta<typeof ReviewerCommentsPopover>;

export default meta;
type Story = StoryObj<typeof meta>;

/** "approved · 3": the verdict, an inline comment and a reply, each jumping to its line. */
export const Approved: Story = {};

/** Requested and silent: the status is plain text, there is nothing to open. */
export const Requested: Story = {
  args: { reviewer: { reviewer: "copilot", is_team: false, requested: true, state: null }, displayName: "GitHub Copilot", reviews: [], threads: [] },
};
