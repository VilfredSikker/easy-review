/** AI Review Arena snapshot contract (C0 — mirrors `er_engine::arena`). */

export type ArenaScope = "branch" | "unstaged" | "staged";

export type RunStatus =
  | "queued"
  | { running: { round: number } }
  | "complete"
  | "cancelled"
  | "failed";

export type ReviewerKind = "model" | "agent";

export type ReviewerRunStatus = "ok" | { failed: { reason: string } };

export type ArenaSeverity = "high" | "med" | "low" | "info";

export type Verdict =
  | "kept"
  | "escalated"
  | { merged: { into: string } }
  | "dropped"
  | "pending";

export type Vote =
  | "propose"
  | "keep"
  | "drop"
  | "merge"
  | "escalate"
  | "lower"
  | "abstain"
  | "flag";

export interface ReviewerRef {
  provider_id: string;
  model_id: string;
}

export interface ArenaConfig {
  reviewers: ReviewerRef[];
  rounds: number;
  arbiter: ReviewerRef;
  auto_accept_threshold: number;
  scope: ArenaScope;
  files?: string[];
}

export interface Reviewer {
  id: string;
  name: string;
  kind: ReviewerKind;
  provider_id: string;
  model_id: string;
  system_prompt: string;
  color: string;
  icon: string;
  tagline: string;
  cost_per_1k_in: number;
  cost_per_1k_out: number;
  avg_latency_ms: number;
  status: ReviewerRunStatus;
}

export interface HumanOverride {
  verdict: Verdict;
  note: string;
  at: string;
}

export interface Ballot {
  reviewer: string;
  vote: Vote;
  note?: string;
  merge_target?: string;
}

export interface RoundLog {
  n: number;
  log: Ballot[];
}

export interface EvidenceItem {
  file: string;
  line_start?: number | null;
  line_end?: number | null;
  note: string;
}

export interface ArenaFinding {
  id: string;
  file: string;
  line?: number | null;
  title: string;
  body: string;
  severity_by_round: Record<number, ArenaSeverity>;
  raised_by: string[];
  verdict: Verdict;
  confidence: number;
  rationale: string;
  rounds: RoundLog[];
  merge_candidates?: string[];
  merged_children?: ArenaFinding[];
  evidence?: EvidenceItem[];
  override?: HumanOverride;
}

export interface CostEstimate {
  tokens_in: number;
  tokens_out: number;
  usd: number;
}

export interface ArenaRun {
  id: string;
  title?: string;
  branch_ref: string;
  base_branch: string;
  scope: ArenaScope;
  diff_hash: string;
  created_at: string;
  completed_at?: string;
  status: RunStatus;
  config: ArenaConfig;
  reviewers: Reviewer[];
  findings: ArenaFinding[];
  cost_estimate: CostEstimate;
}

export interface ArenaRunSummary {
  id: string;
  title?: string;
  branch_ref: string;
  status: RunStatus;
  created_at: string;
  completed_at?: string;
  reviewer_count: number;
  finding_count: number;
}

export interface MatrixRow {
  finding_id: string;
  latest_vote: Record<string, Vote>;
  verdict: Verdict;
  confidence: number;
}

export type FunnelStage = "proposed" | "cross_checked" | "resolved" | "final";

export interface FunnelCounts {
  proposed: number;
  cross_checked: number;
  resolved: number;
  final: number;
}

export interface FunnelStages {
  counts: FunnelCounts;
  exited_at: Record<string, FunnelStage>;
}

export interface ArenaRunSnapshot {
  run: ArenaRun;
  matrix: MatrixRow[];
  funnel: FunnelStages;
}
