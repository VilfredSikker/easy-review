import type { Meta, StoryObj } from "@storybook/svelte";
import DiffToolbar from "$lib/components/DiffToolbar.svelte";
import { commitNote } from "$lib/diffToolbarModel";
import { commitsRich } from "./fixtures";

const noop = () => {};

const meta = {
  title: "Components/DiffToolbar",
  component: DiffToolbar,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof DiffToolbar>;

export default meta;
type Story = StoryObj<typeof meta>;

const base = {
  viewMode: "unified" as const,
  onViewMode: noop,
  hideReviewed: false,
  onHideReviewed: noop,
  note: null,
  onShowAllChanges: noop,
  reviewed: 3,
  total: 10,
};

/** The whole branch: layout, Hide reviewed and the reviewed count. */
export const AllChanges: Story = { args: base };

/** Split layout with reviewed files hidden. */
export const SplitHidingReviewed: Story = {
  args: { ...base, viewMode: "split", hideReviewed: true },
};

/** One commit picked from the rail: its sha chip in the rail's colour, its
 *  title, and the way back. */
export const SingleCommit: Story = {
  args: {
    ...base,
    note: commitNote(commitsRich, commitsRich[1].sha, false),
    reviewed: 0,
    total: 2,
  },
};

/** Mark lines on: the legend tying each colour to its commit. */
export const MarkLinesLegend: Story = {
  args: { ...base, note: commitNote(commitsRich, null, true) },
};
