import type { Meta, StoryObj } from "@storybook/svelte";
import InboxHarness from "$lib/stories/InboxHarness.svelte";
import type { InboxItemSnapshot, ProjectSnapshot } from "$lib/types";

const now = Date.now();

const inboxProjects: ProjectSnapshot[] = [
  {
    id: "discovery-platform",
    name: "discovery-platform",
    root_path: "/Users/vilfred/Projects/discovery-platform",
    remote: "org/discovery-platform",
    is_active: true,
    local_branches: [],
    auto_branches: [],
    saved_prs: [],
    my_prs: [],
    prs_to_review: [],
    recent_prs: [],
    recently_merged: [],
  },
  {
    id: "design-system",
    name: "design-system",
    root_path: "/Users/vilfred/Projects/design-system",
    remote: "org/design-system",
    is_active: false,
    local_branches: [],
    auto_branches: [],
    saved_prs: [],
    my_prs: [],
    prs_to_review: [],
    recent_prs: [],
    recently_merged: [],
  },
];

const allKindsItems: InboxItemSnapshot[] = [
  {
    id: "inbox-comment-1",
    kind: "pr_comment",
    category: "pr_comment",
    severity: "info",
    title: "Comment on your PR #2041",
    body: "This type is a strict subset — just id, name, kind.",
    source: "github",
    target: { pr_number: 2041, project_id: "discovery-platform" },
    created_at_ms: now - 3 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "pr_comment:2041:msg-1",
  },
  {
    id: "inbox-review-received-1",
    kind: "pr_review_received",
    category: "review_received",
    severity: "info",
    title: "alex-p reviewed PR #2041",
    body: "feat/new-search",
    source: "github",
    target: { pr_number: 2041, project_id: "discovery-platform" },
    created_at_ms: now - 8 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "pr_review_received:2041:alex",
  },
  {
    id: "inbox-reply-1",
    kind: "pr_comment_reply",
    category: "comment_reply",
    severity: "info",
    title: "Reply on PR #1987",
    body: "Agreed, let's ship the narrower type.",
    source: "github",
    target: { pr_number: 1987, project_id: "discovery-platform" },
    created_at_ms: now - 12 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "pr_comment_reply:1987:cmt-44",
  },
  {
    id: "inbox-approved-1",
    kind: "pr_review_approved",
    category: "approved",
    severity: "success",
    title: "sam approved PR #88",
    body: "feat/button-variants",
    source: "github",
    target: { pr_number: 88, project_id: "design-system" },
    created_at_ms: now - 18 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "pr_review_approved:88:sam",
  },
  {
    id: "inbox-changes-1",
    kind: "pr_review_changes_requested",
    category: "changes_requested",
    severity: "warning",
    title: "jordan requested changes on PR #2041",
    body: "feat/new-search",
    source: "github",
    target: { pr_number: 2041, project_id: "discovery-platform" },
    created_at_ms: now - 22 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "pr_review_changes_requested:2041:jordan",
  },
  {
    id: "inbox-review-1",
    kind: "review_requested",
    category: "review_requested",
    severity: "info",
    title: "Review requested: feat/new-search",
    body: "alex-p requested your review on PR #2041.",
    source: "github",
    target: { pr_number: 2041, project_id: "discovery-platform" },
    created_at_ms: now - 28 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "review_requested:2041",
  },
  {
    id: "inbox-mention-1",
    kind: "mention",
    category: "mention",
    severity: "info",
    title: "You were mentioned in PR #1987",
    body: "@you what do you think about this approach?",
    source: "github",
    target: { pr_number: 1987, project_id: "discovery-platform" },
    created_at_ms: now - 35 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "mention:1987:cmt-44",
  },
  {
    id: "inbox-ci-1",
    kind: "ci_failed",
    category: "ci",
    severity: "error",
    title: "CI failed on feat/new-search",
    body: "3 checks failed. Click to view the failing run.",
    source: "github",
    target: { branch: "feat/new-search", project_id: "discovery-platform" },
    created_at_ms: now - 40 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "ci_failed:feat-new-search:run-1120",
  },
  {
    id: "inbox-merged-1",
    kind: "pr_merged",
    category: "lifecycle",
    severity: "info",
    title: "PR #2030 merged",
    body: "feat/auth-refactor was merged into main.",
    source: "github",
    target: { pr_number: 2030, project_id: "discovery-platform" },
    created_at_ms: now - 2 * 3_600_000,
    read_at_ms: now - 90 * 60 * 1000,
    dedupe_key: "pr_merged:2030",
  },
  {
    id: "inbox-ai-1",
    kind: "ai_review_done",
    category: "ai",
    severity: "success",
    title: "AI review completed",
    body: "feat/new-search",
    source: "ai",
    target: { branch: "feat/new-search", project_id: "discovery-platform" },
    created_at_ms: now - 3 * 3_600_000,
    read_at_ms: now - 2 * 3_600_000,
    dedupe_key: "ai_review_done:feat-new-search",
  },
];

const meta = {
  title: "Layout/Inbox",
  component: InboxHarness,
  parameters: {
    layout: "fullscreen",
    backgrounds: { default: "dark" },
  },
} satisfies Meta<typeof InboxHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

/**
 * In-rail teaser view: shows the top 2 unread items inline and a
 * "See N more" button. The popover is closed.
 */
export const Teaser: Story = {
  args: {
    inboxItems: allKindsItems,
    projects: inboxProjects,
    autoOpenPopover: false,
  },
};

/**
 * Popover open with category groups and quick-filter chips.
 */
export const PopoverOpen: Story = {
  args: {
    inboxItems: allKindsItems,
    projects: inboxProjects,
    autoOpenPopover: true,
  },
};

/**
 * Empty state: no inbox items — renders the "No notifications" quiet state.
 */
export const Empty: Story = {
  args: {
    inboxItems: [],
    autoOpenPopover: false,
  },
};
