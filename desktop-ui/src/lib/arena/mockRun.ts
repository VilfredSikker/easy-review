import { buildSnapshot } from "$lib/arena/projections";
import type {
  ArenaFinding,
  ArenaRun,
  ArenaRunSnapshot,
  Ballot,
  Reviewer,
} from "$lib/types/arena";

const REVIEWERS: Reviewer[] = [
  {
    id: "general",
    name: "General",
    kind: "agent",
    provider_id: "anthropic",
    model_id: "sonnet-4.5",
    system_prompt: "",
    color: "#ff7a2b",
    icon: "sparkle",
    tagline: "Broad correctness",
    cost_per_1k_in: 0.015,
    cost_per_1k_out: 0.075,
    avg_latency_ms: 12000,
    status: "ok",
  },
  {
    id: "security",
    name: "Security",
    kind: "agent",
    provider_id: "anthropic",
    model_id: "sonnet-4.5",
    system_prompt: "",
    color: "#ff6b6b",
    icon: "shield",
    tagline: "Authn, injection, secrets",
    cost_per_1k_in: 0.015,
    cost_per_1k_out: 0.075,
    avg_latency_ms: 12000,
    status: "ok",
  },
  {
    id: "perf",
    name: "Performance",
    kind: "agent",
    provider_id: "anthropic",
    model_id: "sonnet-4.5",
    system_prompt: "",
    color: "#7f87ff",
    icon: "lightning",
    tagline: "Hot paths",
    cost_per_1k_in: 0.015,
    cost_per_1k_out: 0.075,
    avg_latency_ms: 12000,
    status: "ok",
  },
  {
    id: "style",
    name: "Style",
    kind: "agent",
    provider_id: "anthropic",
    model_id: "haiku-4.5",
    system_prompt: "",
    color: "#4ec9a4",
    icon: "brush",
    tagline: "Naming & idioms",
    cost_per_1k_in: 0.004,
    cost_per_1k_out: 0.02,
    avg_latency_ms: 5000,
    status: "ok",
  },
  {
    id: "tests",
    name: "Tests",
    kind: "agent",
    provider_id: "anthropic",
    model_id: "sonnet-4.5",
    system_prompt: "",
    color: "#ffc457",
    icon: "tube",
    tagline: "Coverage gaps",
    cost_per_1k_in: 0.015,
    cost_per_1k_out: 0.075,
    avg_latency_ms: 12000,
    status: "ok",
  },
];

function ballot(reviewer: string, vote: Ballot["vote"], note = "", merge_target?: string): Ballot {
  return { reviewer, vote, note, merge_target };
}

const FINDINGS: ArenaFinding[] = [
  {
    id: "f-mapkey",
    file: "variant-warning-copy.ts",
    line: 67,
    title: "Mapped key uses raw user input",
    body: "User-controlled key flows into property access without sanitisation — prototype pollution surface.",
    raised_by: ["general", "security"],
    severity_by_round: { 1: "high", 2: "high", 3: "high" },
    verdict: "kept",
    confidence: 0.94,
    rationale:
      "Two agents independently flagged it; security strengthened the framing. No dissent.",
    rounds: [
      {
        n: 1,
        log: [
          ballot("general", "propose", "Mapped key uses raw user input — risks collision."),
          ballot("security", "propose", "User-controlled key without sanitisation."),
        ],
      },
      {
        n: 2,
        log: [
          ballot("general", "keep", "Security framing is more accurate."),
          ballot("security", "keep", "Confirmed exploitable."),
          ballot("perf", "abstain"),
          ballot("style", "abstain"),
          ballot("tests", "flag", "No test for malicious-key path."),
        ],
      },
      { n: 3, log: [ballot("general", "keep", "Merged wording from Security.")] },
    ],
    merge_candidates: [],
  },
  {
    id: "f-injection",
    file: "experiment-template-resolution.ts",
    line: 91,
    title: "Template name interpolated into raw SQL",
    body: "Switch to parameterised binding.",
    raised_by: ["security"],
    severity_by_round: { 1: "high", 2: "high", 3: "high" },
    verdict: "kept",
    confidence: 0.88,
    rationale: "Cross-check confirmed reachable from user-facing form.",
    rounds: [
      { n: 1, log: [ballot("security", "propose", "Interpolated into raw query string.")] },
      {
        n: 2,
        log: [
          ballot("general", "keep", "Reachable from user form."),
          ballot("security", "keep"),
          ballot("perf", "abstain"),
          ballot("style", "abstain"),
        ],
      },
    ],
  },
  {
    id: "f-undef-sev",
    file: "variant-warning-copy.ts",
    line: 42,
    title: "Fallback returns undefined for missing severity",
    body: "Callers expect a string; returning undefined breaks downstream string ops.",
    raised_by: ["general", "tests"],
    severity_by_round: { 1: "high", 2: "med", 3: "med" },
    verdict: "kept",
    confidence: 0.81,
    rationale: "Real bug, downgraded after Tests showed caller null-checks.",
    rounds: [
      {
        n: 1,
        log: [
          ballot("general", "propose", "Fallback returns undefined."),
          ballot("tests", "propose", "No tests for undefined-severity branch."),
        ],
      },
      {
        n: 2,
        log: [
          ballot("general", "lower", "Only one caller; it null-checks."),
          ballot("tests", "keep"),
          ballot("security", "abstain"),
        ],
      },
    ],
  },
  {
    id: "f-nplusone",
    file: "PropertyMediaEditor.svelte",
    line: 88,
    title: "N+1 fetch in render loop",
    body: "Each row triggers a separate getMediaPreview call.",
    raised_by: ["perf"],
    severity_by_round: { 1: "med", 2: "med", 3: "med" },
    verdict: "kept",
    confidence: 0.77,
    rationale: "Perf flagged; General confirmed batch endpoint exists.",
    rounds: [
      { n: 1, log: [ballot("perf", "propose", "N+1 fetch in render loop.")] },
      {
        n: 2,
        log: [
          ballot("general", "keep", "Batch endpoint at /media/preview-batch."),
          ballot("perf", "keep"),
        ],
      },
    ],
  },
  {
    id: "f-log-secret",
    file: "analysis-runner.ts",
    line: 204,
    title: "Auth token logged on failure path",
    body: "Resolver failure path logs the bearer token.",
    raised_by: ["security"],
    severity_by_round: { 1: "med", 2: "high", 3: "high" },
    verdict: "escalated",
    confidence: 0.91,
    rationale: "General escalated — logs ship to Datadog.",
    rounds: [
      { n: 1, log: [ballot("security", "propose", "Auth token logged on failure path.")] },
      {
        n: 2,
        log: [
          ballot("general", "escalate", "Logs ship to shared sink — bump to High."),
          ballot("security", "keep"),
        ],
      },
    ],
  },
  {
    id: "f-realloc",
    file: "experiment-template-resolution.ts",
    line: 230,
    title: "Array realloc in hot path",
    body: "New array allocated per call; pre-size or reuse.",
    raised_by: ["perf"],
    severity_by_round: { 1: "low", 2: "low" },
    verdict: "dropped",
    confidence: 0.42,
    rationale: "Not on a measured hot path; Perf conceded.",
    rounds: [
      { n: 1, log: [ballot("perf", "propose", "New array per call in hot path.")] },
      {
        n: 2,
        log: [
          ballot("general", "drop", "Profiler shows <10x per session."),
          ballot("perf", "drop", "Concede; dropping."),
        ],
      },
    ],
  },
];

export const MOCK_ARENA_RUN: ArenaRun = {
  id: "arena-2026-05-27-001",
  title: "Branch review — feature/media-editor",
  branch_ref: "feature/media-editor",
  base_branch: "main",
  scope: "branch",
  diff_hash: "mock-diff-hash",
  created_at: "2026-05-27T12:00:00Z",
  completed_at: "2026-05-27T12:00:48Z",
  status: "complete",
  config: {
    reviewers: REVIEWERS.map((r) => ({
      provider_id: r.provider_id,
      model_id: r.model_id,
    })),
    rounds: 3,
    arbiter: { provider_id: "anthropic", model_id: "opus-4.8" },
    auto_accept_threshold: 0.75,
    scope: "branch",
  },
  reviewers: REVIEWERS,
  findings: FINDINGS,
  cost_estimate: { tokens_in: 120_000, tokens_out: 18_000, usd: 2.4 },
};

export const MOCK_ARENA_SNAPSHOT: ArenaRunSnapshot = buildSnapshot(MOCK_ARENA_RUN);

const MODEL_REVIEWERS: Reviewer[] = [
  {
    id: "claude-sonnet",
    name: "Sonnet 4.6",
    kind: "model",
    provider_id: "claude",
    model_id: "claude-sonnet-4.6",
    system_prompt: "",
    color: "#ff7a2b",
    icon: "cube",
    tagline: "Anthropic",
    cost_per_1k_in: 0.015,
    cost_per_1k_out: 0.075,
    avg_latency_ms: 12_000,
    status: "ok",
  },
  {
    id: "openai-gpt",
    name: "GPT-5.4",
    kind: "model",
    provider_id: "codex",
    model_id: "gpt-5.4",
    system_prompt: "",
    color: "#4ec9a4",
    icon: "cube",
    tagline: "OpenAI",
    cost_per_1k_in: 0.012,
    cost_per_1k_out: 0.06,
    avg_latency_ms: 10_000,
    status: "ok",
  },
  {
    id: "cursor-composer",
    name: "Composer 2.5",
    kind: "model",
    provider_id: "cursor",
    model_id: "cursor-composer-2.5",
    system_prompt: "",
    color: "#7f87ff",
    icon: "cube",
    tagline: "Cursor",
    cost_per_1k_in: 0.01,
    cost_per_1k_out: 0.05,
    avg_latency_ms: 8_000,
    status: "ok",
  },
];

/** Snapshot matching a 3-model arena run (matrix/funnel UI). */
export const MOCK_MODEL_ARENA_SNAPSHOT: ArenaRunSnapshot = buildSnapshot({
  ...MOCK_ARENA_RUN,
  id: "arena-mock-models-001",
  title: "Standard · Sonnet × GPT × Composer",
  reviewers: MODEL_REVIEWERS,
  config: {
    ...MOCK_ARENA_RUN.config,
    reviewers: MODEL_REVIEWERS.map((r) => ({
      provider_id: r.provider_id,
      model_id: r.model_id,
    })),
    arbiter: { provider_id: "claude", model_id: "claude-opus-4" },
  },
  findings: FINDINGS.slice(0, 5).map((f, i) =>
    i === 0
      ? {
          ...f,
          rationale:
            f.rationale +
            " Round-2 majority (claude-sonnet-4.6, cursor-composer-2.5) kept this: swap walks a pre-resolve snapshot while resolve/dedupe can remove rows. Security framing was strengthened; style and perf abstained. This paragraph is intentionally long so Storybook can exercise expanded rationale UI.",
        }
      : f,
  ),
});

export function makeRunningSnapshot(round: number): ArenaRunSnapshot {
  return buildSnapshot({
    ...MOCK_MODEL_ARENA_SNAPSHOT.run,
    status: { running: { round } },
    completed_at: undefined,
  });
}

export function reviewerById(id: string): Reviewer | undefined {
  return REVIEWERS.find((r) => r.id === id);
}
