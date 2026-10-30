import type { Meta, StoryObj } from "@storybook/svelte";
import TopBarHarness from "./TopBarHarness.svelte";
import { reviewFlowFixture, richSnapshot } from "./fixtures";
import type { AppSnapshot, BackgroundTaskSnapshot, ReviewFlowTaskProgress } from "$lib/types";

const meta = {
  title: "Shell/TopBar",
  component: TopBarHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "rail" } },
} satisfies Meta<typeof TopBarHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

const flowTask = (
  id: string,
  stage: ReviewFlowTaskProgress["stage"],
  status: ReviewFlowTaskProgress["status"],
  extra: Partial<ReviewFlowTaskProgress> = {},
): ReviewFlowTaskProgress => ({
  id,
  stage,
  lens: stage === "finding" ? id : null,
  label: stage === "triage" ? "Triage" : stage === "verifying" ? "Arbiter" : id,
  purpose: stage === "triage" ? "Choose review depth and lenses" : `Look for ${id} problems`,
  provider_id: "openai",
  provider_label: "OpenAI",
  model_id: "gpt-test",
  model_label: "GPT Test",
  status,
  error: null,
  background_task_id: `bg-${id}`,
  round: null,
  total_rounds: null,
  ...extra,
});

const bgTask = (id: string, status: string): BackgroundTaskSnapshot => ({
  id,
  kind: "review",
  label: "Diagrams",
  target_label: "PR #154",
  scope: "pr",
  repo_root: "/repo",
  branch_label: richSnapshot.branch,
  status,
  started_at_ms: Date.now() - 42_000,
  recent_log: [
    { command_name: "diagrams", source: "stdout", text: "→ Read src/lib/types.ts" },
    { command_name: "diagrams", source: "stdout", text: "Drafting the subsystem map." },
  ],
});

const reviewFinding: AppSnapshot = {
  ...richSnapshot,
  reviewed_count: 5,
  total_count: 12,
  background_tasks: [bgTask("bg-other", "running")],
  review_flow: reviewFlowFixture("finding", {
    tasks: [
      flowTask("triage", "triage", "done"),
      flowTask("general", "finding", "done"),
      flowTask("security", "finding", "running", { round: 2, total_rounds: 3 }),
      flowTask("database", "finding", "failed", { error: "Provider timed out after 120s" }),
    ],
  }),
};

export const Idle: Story = { args: { snapshot: { ...richSnapshot, reviewed_count: 3, total_count: 12 } } };

export const ReviewFinding: Story = { args: { snapshot: reviewFinding } };

export const DeepNeedsConfirmation: Story = {
  args: {
    snapshot: {
      ...richSnapshot,
      review_flow: reviewFlowFixture("awaiting_confirmation", {
        estimated_cost: 0.82,
        estimated_latency_seconds: 240,
        tasks: [flowTask("triage", "triage", "done")],
      }),
    },
  },
};

export const DeepDoneAwaitingAccept: Story = {
  args: {
    snapshot: {
      ...richSnapshot,
      review_flow: reviewFlowFixture("done", {
        depth: "deep",
        tasks: [flowTask("triage", "triage", "done"), flowTask("arbiter", "verifying", "done")],
      }),
    },
  },
};

export const OtherJobsOnly: Story = {
  args: { snapshot: { ...richSnapshot, background_tasks: [bgTask("bg-1", "running"), bgTask("bg-2", "queued")] } },
};

export const StaleDiff: Story = {
  args: {
    snapshot: {
      ...richSnapshot,
      mode: "pr",
      diff_stale: { kind: "pr_head", message: "PR head moved 2 commits", built_from_oid: "a", latest_oid: "b", commits_behind: 2 },
    },
  },
};

export const Narrow: Story = { args: { snapshot: reviewFinding, width: 480 } };
