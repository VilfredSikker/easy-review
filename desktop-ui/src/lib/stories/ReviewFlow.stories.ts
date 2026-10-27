import type { Meta, StoryObj } from "@storybook/svelte";
import ReviewFlowHarness from "./ReviewFlowHarness.svelte";
import { reviewFlowFixture } from "./fixtures";
import type { ReviewFlowTaskProgress } from "$lib/types";

const task = (
  id: string,
  stage: ReviewFlowTaskProgress["stage"],
  status: ReviewFlowTaskProgress["status"],
  extra: Partial<ReviewFlowTaskProgress> = {},
): ReviewFlowTaskProgress => ({
  id,
  stage,
  lens: stage === "finding" ? id.split("-")[0] : null,
  label: id,
  purpose: `${stage} work for ${id}`,
  provider_id: "openai",
  provider_label: "OpenAI",
  model_id: "gpt-test",
  model_label: "GPT Test",
  status,
  error: null,
  background_task_id: id,
  round: null,
  total_rounds: null,
  ...extra,
});

const meta = {
  title: "RightPanel/ReviewFlow",
  component: ReviewFlowHarness,
  parameters: { layout: "padded", backgrounds: { default: "rail" } },
} satisfies Meta<typeof ReviewFlowHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Idle: Story = { args: { flow: null } };
export const Triaging: Story = {
  args: {
    flow: reviewFlowFixture("triaging", {
      tasks: [task("triage", "triage", "running", { purpose: "Choose reviewers for this diff" })],
    }),
  },
};
export const StandardPartialFailure: Story = {
  args: {
    flow: reviewFlowFixture("finding", {
      tasks: [
        task("security", "finding", "running", { purpose: "Find security issues" }),
        task("database", "finding", "failed", { error: "Provider request timed out" }),
        task("reliability", "finding", "queued", { purpose: "Check failure handling" }),
      ],
    }),
  },
};
export const QueuedReviewers: Story = {
  args: {
    flow: reviewFlowFixture("finding", {
      tasks: [
        task("security", "finding", "running"),
        task("database", "finding", "queued"),
        task("reliability", "finding", "waiting"),
      ],
    }),
  },
};
export const DeepCrosscheck: Story = {
  args: {
    flow: reviewFlowFixture("verifying", {
      depth: "deep",
      tasks: [
        task("finding-r1", "finding", "done", { label: "Find R1 · security", round: 1, total_rounds: 2 }),
        task("finding-r2", "finding", "done", { label: "Find R2 · database", round: 2, total_rounds: 2 }),
        task("crosscheck-r2", "verifying", "running", {
          label: "Crosscheck R2 · security",
          purpose: "Check the database findings against the diff",
          round: 2,
          total_rounds: 2,
        }),
        task("arbiter", "verifying", "waiting", { label: "Arbiter", purpose: "Produce final verdicts" }),
      ],
    }),
  },
};
export const Done: Story = {
  args: {
    flow: reviewFlowFixture("done", {
      tasks: [
        task("triage", "triage", "done"),
        task("security", "finding", "done"),
        task("arbiter", "verifying", "done"),
      ],
    }),
  },
};
export const AwaitingConfirmation: Story = { args: { flow: reviewFlowFixture("awaiting_confirmation") } };
export const DeepPlanConfirmation: Story = {
  args: {
    showTriage: true,
    flow: reviewFlowFixture("awaiting_confirmation", {
      tasks: [
        task("triage", "triage", "done", { purpose: "Choose the full reviewer plan" }),
        ...Array.from({ length: 32 }, (_, index) => task(
          `finding-r${Math.floor(index / 8) + 1}-${index + 1}`,
          "finding",
          "waiting",
          {
            lens: ["security", "reliability", "database", "architecture"][index % 4],
            round: Math.floor(index / 8) + 1,
            total_rounds: 4,
            purpose: `Find and cross-check issue candidates for round ${Math.floor(index / 8) + 1}`,
          },
        )),
        task("arbiter", "verifying", "waiting", { label: "Arbiter", purpose: "Produce final verdicts" }),
      ],
    }),
  },
};
export const Draining: Story = { args: { flow: reviewFlowFixture("failed", { draining: true }) } };
export const Failure: Story = { args: { flow: reviewFlowFixture("failed") } };
export const AwaitingAcceptance: Story = {
  args: {
    flow: reviewFlowFixture("done", {
      depth: "deep",
      kept: 0,
      dropped: 0,
      awaiting_accept: 3,
      successful_finder_providers: ["openai"],
      successful_arbiter_provider: "openai",
      providers_used: ["openai"],
    }),
  },
};
