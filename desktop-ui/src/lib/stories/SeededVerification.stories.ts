import type { Meta, StoryObj } from "@storybook/svelte";
import type { ArenaRunSnapshot, RunStatus, Verdict } from "$lib/types/arena";
import { buildSnapshot } from "$lib/arena/projections";
import { MOCK_ARENA_SNAPSHOT } from "$lib/arena/mockRun";
import ArenaHarness from "./ArenaHarness.svelte";

function seededVerificationSnapshot(status: RunStatus): ArenaRunSnapshot {
  const source = MOCK_ARENA_SNAPSHOT.run;
  const contributor = source.reviewers.find((reviewer) => reviewer.id === "security") ?? source.reviewers[0]!;
  const verdicts: Verdict[] = [
    "kept",
    "escalated",
    { merged: { into: "f-mapkey" } },
    "dropped",
    "pending",
  ];
  const run = {
    ...source,
    id: `verification-${status === "complete" ? "complete" : "failed"}-single-lens`,
    title: "Single lens arbiter verification",
    status,
    completed_at: status === "complete" ? "2026-05-27T12:01:00Z" : undefined,
    config: {
      ...source.config,
      run_kind: "seeded" as const,
      rounds: 1,
      reviewers: [{ provider_id: contributor.provider_id, model_id: contributor.model_id }],
      arbiter: { provider_id: "codex", model_id: "gpt-6-astra" },
    },
    reviewers: [contributor],
    findings: verdicts.map((verdict, index) => {
      const finding = source.findings[index] ?? source.findings[0]!;
      return {
        ...finding,
        id: index < source.findings.length ? finding.id : `verification-pending-${index}`,
        title: verdict === "pending" ? "Awaiting verification" : finding.title,
        rationale: verdict === "pending" ? "" : finding.rationale,
        raised_by: [contributor.id],
        verdict,
        accepted_at: undefined,
      };
    }),
  };
  return {
    ...buildSnapshot(run),
    arbiter: { label: "Codex GPT-6 Astra", provider_id: "codex", model_id: "gpt-6-astra" },
  };
}

const meta = {
  title: "Arena/Seeded verification",
  component: ArenaHarness,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ArenaHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CompleteSingleLens: Story = {
  args: { snapshot: seededVerificationSnapshot("complete") },
};

export const FailedSingleLens: Story = {
  args: { snapshot: seededVerificationSnapshot("failed") },
};
