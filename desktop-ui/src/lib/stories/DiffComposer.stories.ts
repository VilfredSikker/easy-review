import type { Meta, StoryObj } from "@storybook/svelte";
import DiffComposerHarness from "./DiffComposerHarness.svelte";
import { richSnapshot } from "./fixtures";

const meta = {
  title: "Components/DiffComposer",
  component: DiffComposerHarness,
  parameters: { layout: "centered", backgrounds: { default: "app" } },
} satisfies Meta<typeof DiffComposerHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

/**
 * In flow, directly below the selected lines — the normal case. The card owns
 * real layout space, so nothing below it is covered.
 */
export const InFlow: Story = {
  args: { placement: { kind: "flow" }, text: "" },
};

/** Mid-draft, with the question/composer kind toggle showing the active kind. */
export const InFlowDraft: Story = {
  args: { placement: { kind: "flow" }, kind: "note", text: "Hand this to an agent: rename `label`." },
};

/** Split view: the card takes the pane the selection was made in. */
export const InFlowSplit: Story = {
  args: { placement: { kind: "flow" }, splitPane: "new" },
};

/**
 * Floating fallback — the anchor row is outside the rendered window, so the
 * card holds its content position and scrolls with the diff.
 */
export const FloatingFallback: Story = {
  args: { placement: { kind: "absolute", topPx: 80 }, text: "Draft kept while scrolled away." },
};

/** Docked fallback — no rendered anchor row (compacted file, lazy stub). */
export const DockedFallback: Story = {
  args: { placement: { kind: "sticky" } },
};

/**
 * A GitHub comment on a PR: "Add single comment" posts now, "Start a review"
 * adds it to the pending review that Review submits.
 */
export const GitHubCommentOnPr: Story = {
  args: {
    placement: { kind: "flow" },
    text: "Does this need a guard for the empty list?",
    snapshot: { ...richSnapshot, detected_pr_number: 1756 },
  },
};

/** No PR for the branch: both GitHub actions are disabled with the reason. */
export const GitHubCommentNoPr: Story = {
  args: {
    placement: { kind: "flow" },
    text: "Does this need a guard for the empty list?",
    snapshot: { ...richSnapshot, detected_pr_number: null, github: null },
  },
};
