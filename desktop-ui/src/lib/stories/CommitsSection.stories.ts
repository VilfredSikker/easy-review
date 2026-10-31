import type { Meta, StoryObj } from "@storybook/svelte";
import CommitsSection from "$lib/components/CommitsSection.svelte";
import { commitsRich } from "./fixtures";

const noop = () => {};

const meta = {
  title: "Components/CommitsSection",
  component: CommitsSection,
  parameters: { layout: "padded", backgrounds: { default: "rail" } },
} satisfies Meta<typeof CommitsSection>;

export default meta;
type Story = StoryObj<typeof meta>;

const base = {
  commits: commitsRich,
  selectedSha: null,
  selectable: true,
  onSelectCommit: noop,
  onAllChanges: noop,
  reviewedCount: 3,
  totalCount: 10,
};

/** Local branch view: "Branch" row active with its totals, the unpushed chip on
 *  the newest commit, and the merge's 0/0. */
export const LocalBranch: Story = {
  args: {
    ...base,
    allLabel: "Branch",
    allActive: true,
    allStat: { additions: 59, deletions: 12 },
    scopeLabel: "Local branch",
  },
};

/** A commit picked from the PR diff view: the footer counts the History
 *  bucket's own marks and names the commit. */
export const CommitSelected: Story = {
  args: {
    ...base,
    selectedSha: commitsRich[1].sha,
    allLabel: "All changes",
    allActive: false,
    allStat: null,
    scopeLabel: `Commit ${commitsRich[1].sha.slice(0, 7)}`,
    reviewedCount: 0,
    totalCount: 2,
  },
};

/** Remote PR tab: commits are display only, and a PR commit outside the local
 *  object DB shows no +/- rather than +0 −0. */
export const RemotePrDisplayOnly: Story = {
  args: {
    ...base,
    commits: commitsRich.map((c, i) => (i === 0 ? { ...c, additions: null, deletions: null, unpushed: false } : c)),
    selectable: false,
    allLabel: "All changes",
    allActive: true,
    allStat: { additions: 59, deletions: 12 },
    scopeLabel: "PR Diff",
  },
};

/** Mark lines on: the switch in the Commits heading drives the inset bars in
 *  the diff and the legend in the toolbar, coloured from the same series. */
export const MarkLinesOn: Story = {
  args: {
    ...base,
    allLabel: "All changes",
    allActive: true,
    allStat: { additions: 59, deletions: 12 },
    scopeLabel: "PR Diff",
    markLines: true,
    onMarkLines: noop,
  },
};
