import type { Meta, StoryObj } from "@storybook/svelte";
import OverviewFindings from "$lib/components/overview/OverviewFindings.svelte";
import OverviewTour from "$lib/components/overview/OverviewTour.svelte";
import OverviewActivity from "$lib/components/overview/OverviewActivity.svelte";
import EmptyBucket from "$lib/components/overview/EmptyBucket.svelte";
import type { AiSnapshot, TourSnapshot } from "$lib/types";
import {
  aiEmpty,
  aiWithFindings,
  githubStatusOpen,
  localBranchEmptyBucketSnapshot,
  localBranchSharedTourSnapshot,
} from "./fixtures";

const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();

const reviewed: AiSnapshot = {
  ...aiWithFindings,
  review_created_at: minutesAgo(19),
  findings: aiWithFindings.findings.map((f, i) => ({
    ...f,
    confidence: i === 0 ? "confirmed" : f.confidence,
    raised_by: i === 1 ? ["general", "database"] : f.raised_by,
    providers: i === 1 ? ["anthropic", "openai"] : ["anthropic"],
    message_markdown: i === 0 ? "Callers interpolate the result into a toast, so `undefined` reaches the UI." : f.message_markdown,
  })),
  resolved_findings: [
    { ...aiWithFindings.findings[3], id: "resolved-1", resolved: true, title: "Onboarding test still listed the old filename" },
  ],
};

const tour: TourSnapshot = {
  available: true,
  fresh: true,
  scope: "pr",
  title: "Reorder migrations",
  overviewMarkdown: "",
  createdAt: minutesAgo(12),
  pillars: [
    { id: "p1", title: "Restore migration order", descriptionMarkdown: "", summaryMarkdown: "Three migrations move after the staging head so `deploy-db` stops rejecting them.", importance: 1, foundation: true, files: [{ path: "a.sql", reason: "", findingIds: [] }], reviewedCount: 2, totalCount: 4 },
    { id: "p2", title: "Update migration references", descriptionMarkdown: "Tests that load migrations by filename point at the renamed files.", importance: 2, foundation: false, files: [], reviewedCount: 2, totalCount: 2 },
    { id: "p3", title: "Guard the order in CI", descriptionMarkdown: "", summaryMarkdown: "A new CI step fails a PR whose migrations are older than the base branch.", importance: 3, foundation: false, files: [], reviewedCount: 0, totalCount: 2 },
  ],
};

const meta = {
  title: "Overview/Sections",
  parameters: { layout: "padded", backgrounds: { default: "app" } },
} satisfies Meta;

export default meta;

export const Findings: StoryObj<typeof OverviewFindings> = {
  render: (args) => ({ Component: OverviewFindings, props: args }),
  args: { ai: reviewed, onPrView: true },
};

export const FindingsStale: StoryObj<typeof OverviewFindings> = {
  render: (args) => ({ Component: OverviewFindings, props: args }),
  args: { ai: { ...reviewed, fresh: false, stale_reason: "Built from 7be0d14" }, onPrView: false },
};

export const Tour: StoryObj<typeof OverviewTour> = {
  render: (args) => ({ Component: OverviewTour, props: args }),
  args: { tour },
};

export const TourMissing: StoryObj<typeof OverviewTour> = {
  render: (args) => ({ Component: OverviewTour, props: args }),
  args: { tour: null },
};

export const Activity: StoryObj<typeof OverviewActivity> = {
  render: (args) => ({ Component: OverviewActivity, props: args }),
  args: {
    ai: reviewed,
    github: {
      ...githubStatusOpen,
      created_at: minutesAgo(21),
      comments_count: 3,
      recent_comments: [
        { author: "maria-c", body: "Does `just reseed` cover the shared dev DB?", created_at: minutesAgo(15), url: "https://github.com/owner/repo/pull/123#c1" },
      ],
      recent_reviews: [
        { author: "martin", state: "APPROVED", body: "", submitted_at: minutesAgo(12) },
      ],
    },
  },
};

export const EmptyBucketLocal: StoryObj<typeof EmptyBucket> = {
  render: (args) => ({ Component: EmptyBucket, props: args }),
  args: { snapshot: { ...localBranchEmptyBucketSnapshot, ai: aiEmpty } },
};

export const EmptyBucketSharedTour: StoryObj<typeof EmptyBucket> = {
  render: (args) => ({ Component: EmptyBucket, props: args }),
  args: {
    snapshot: {
      ...localBranchSharedTourSnapshot,
      ai: aiEmpty,
      tour,
    },
  },
};
