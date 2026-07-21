import type { Meta, StoryObj } from "@storybook/svelte";
import AiReviewCard from "$lib/components/AiReviewCard.svelte";
import {
  aiWithFindings,
  aiEmpty,
  aiProfessorOnly,
  aiMultiAgent,
  aiFileRisksOnly,
} from "./fixtures";

const meta = {
  title: "RightPanel/AiReviewCard",
  component: AiReviewCard,
  parameters: { layout: "padded", backgrounds: { default: "rail" } },
} satisfies Meta<typeof AiReviewCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Fresh: Story = { args: { ai: aiWithFindings } };
export const Stale: Story = { args: { ai: { ...aiWithFindings, fresh: false } } };
export const NoFindings: Story = { args: { ai: aiEmpty } };
export const FileRisksOnly: Story = { args: { ai: aiFileRisksOnly } };
export const LongSummaryCollapsed: Story = {
  args: {
    ai: {
      ...aiWithFindings,
      summary_markdown:
        "This is a deliberately long summary to exercise collapsed rendering. It includes multiple sentences about changed behavior, potential regressions, and review hotspots so the card shows a compact two-line preview until expanded by the user.",
    },
  },
};
export const StaleWithReason: Story = {
  args: {
    ai: {
      ...aiWithFindings,
      fresh: false,
      stale_reason: "Review was generated for an older diff. Re-run or validate the review.",
    },
  },
};

export const ProfessorOnly: Story = { args: { ai: aiProfessorOnly } };

export const MultiAgent: Story = { args: { ai: aiMultiAgent } };
