import type { Meta, StoryObj } from "@storybook/svelte";
import InboxHarness from "$lib/stories/InboxHarness.svelte";
import type { InboxItemSnapshot, ProjectSnapshot } from "$lib/types";

// ─── fixture items covering all supported inbox kinds ───────────────────────
// LeftSidebar.inboxKindMeta() switches on: "pr_merged"|"merged",
// "ci_failed"|"ci-fail"|"check_failed", "review_requested"|"review",
// "new_comment"|"comment", "mention". Anything else falls through to the
// severity branch ("error"|"warning") or the default briefcase icon.

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
  // ── unread ──────────────────────────────────────────────────────────────
  {
    id: "inbox-review-1",
    kind: "review_requested",
    severity: "info",
    title: "Review requested: feat/new-search",
    body: "alex-p requested your review on PR #2041.",
    source: "github",
    target: { pr_number: 2041, project_id: "discovery-platform" },
    created_at_ms: now - 3 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "review_requested:2041",
  },
  {
    id: "inbox-comment-1",
    kind: "new_comment",
    severity: "info",
    title: "New comment on PR #2041",
    body: "This type is a strict subset — just id, name, kind.",
    source: "github",
    target: { pr_number: 2041, project_id: "discovery-platform" },
    created_at_ms: now - 8 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "new_comment:2041:msg-1",
  },
  {
    id: "inbox-mention-1",
    kind: "mention",
    severity: "info",
    title: "You were mentioned in PR #1987",
    body: "@you what do you think about this approach?",
    source: "github",
    target: { pr_number: 1987, project_id: "discovery-platform" },
    created_at_ms: now - 15 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "mention:1987:cmt-44",
  },
  {
    id: "inbox-ci-1",
    kind: "ci_failed",
    severity: "error",
    title: "CI failed on feat/new-search",
    body: "3 checks failed. Click to view the failing run.",
    source: "github",
    target: { branch: "feat/new-search", project_id: "discovery-platform" },
    created_at_ms: now - 28 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "ci_failed:feat-new-search:run-1120",
  },
  // ── read ─────────────────────────────────────────────────────────────────
  {
    id: "inbox-merged-1",
    kind: "pr_merged",
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
    id: "inbox-comment-2",
    kind: "comment",
    severity: "info",
    title: "Comment resolved on PR #2030",
    body: "Thread marked as resolved.",
    source: "github",
    target: { pr_number: 2030, project_id: "discovery-platform" },
    created_at_ms: now - 3 * 3_600_000,
    read_at_ms: now - 2 * 3_600_000,
    dedupe_key: "comment:2030:thread-8",
  },
  {
    id: "inbox-review-design-1",
    kind: "review_requested",
    severity: "info",
    title: "Review requested: feat/button-variants",
    body: "sam requested your review on PR #88.",
    source: "github",
    target: { pr_number: 88, project_id: "design-system" },
    created_at_ms: now - 45 * 60 * 1000,
    read_at_ms: null,
    dedupe_key: "review_requested:88",
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
 * Popover open: same data as Teaser. On mount the harness queries the DOM
 * for the "Inbox" eyebrow button and clicks it programmatically so the
 * popover opens. (@storybook/test is not installed in this project, so we
 * use autoOpenPopover=true instead of a play() function.)
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
