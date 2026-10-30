import { commitColorVar } from "$lib/commitColors";
import type {
  ArenaFinding,
  ArenaRunDigest,
  ArenaRunKind,
  ArenaRunSnapshot,
  ArenaRunSummary,
  ArenaSeverity,
  Ballot,
  ReviewerRef,
  RunStatus,
  Verdict,
  Vote,
} from "$lib/types/arena";

/** Round number the engine records the arbiter's ballots under. */
export const ARBITER_ROUND = 255;

export interface OverlayAccess {
  /** Flow runs belong to the review flow; their results are read through its origin, not `arena_get`. */
  flowManaged: boolean;
  /** A flow run opened outside flow acceptance: nothing on the page may change it. */
  readOnlyFlow: boolean;
  readOnly: boolean;
  /**
   * "Keep anyway" goes through `arena_override`, which resolves the run by its
   * id against the tabs' current buckets rather than the flow's captured
   * `erDir`, so a flow-managed run never offers it.
   */
  keepAnyway: boolean;
}

export function overlayAccess(opts: {
  flowAcceptance: boolean;
  runKind: ArenaRunKind | undefined;
  readOnlyVerification: boolean;
}): OverlayAccess {
  const flowManaged = opts.flowAcceptance || opts.runKind === "review_flow";
  const readOnlyFlow = flowManaged && !opts.flowAcceptance;
  const readOnly = readOnlyFlow || opts.readOnlyVerification;
  return { flowManaged, readOnlyFlow, readOnly, keepAnyway: !readOnly && !flowManaged };
}

export type VerdictKey = "kept" | "escalated" | "merged" | "dropped" | "pending";

export function verdictKey(verdict: Verdict): VerdictKey {
  if (typeof verdict === "object") return "merged";
  return verdict;
}

export const VERDICT_LABEL: Record<VerdictKey, string> = {
  kept: "Kept",
  escalated: "Escalated",
  merged: "Merged",
  dropped: "Dropped",
  pending: "Pending",
};

export const VOTE_META: Record<Vote, { glyph: string; label: string }> = {
  propose: { glyph: "◆", label: "Propose" },
  keep: { glyph: "✓", label: "Keep" },
  drop: { glyph: "✕", label: "Drop" },
  merge: { glyph: "⇢", label: "Merge" },
  escalate: { glyph: "▲", label: "Escalate" },
  lower: { glyph: "▼", label: "Lower" },
  flag: { glyph: "⚑", label: "Flag" },
  abstain: { glyph: "·", label: "Abstain" },
};

/** Legend order follows a finding's life: raised, judged, then the rarer calls. */
export const VOTE_ORDER: Vote[] = ["propose", "keep", "drop", "merge", "escalate", "lower", "flag", "abstain"];

/**
 * Short labels ("F" plus the 1-based position) in the order the run lists its findings. Engine ids
 * are slugs that do not fit a matrix cell, and every view of one run has to
 * agree on the label, so it is derived once from the finding order.
 */
export function findingLabels(findings: Pick<ArenaFinding, "id">[]): Map<string, string> {
  return new Map(findings.map((f, i) => [f.id, `F${i + 1}`]));
}

/** Reviewer rounds that actually ran (the arbiter's round excluded). */
export function roundsRun(findings: Pick<ArenaFinding, "rounds">[]): number {
  let max = 0;
  for (const f of findings) {
    for (const r of f.rounds) {
      if (r.n !== ARBITER_ROUND && r.n > max && r.log.length > 0) max = r.n;
    }
  }
  return max;
}

/** Round numbers a run's views offer: the configured count, widened if more ran. */
export function roundNumbers(snapshot: ArenaRunSnapshot): number[] {
  const n = Math.max(snapshot.run.config.rounds ?? 1, roundsRun(snapshot.run.findings), 1);
  return Array.from({ length: n }, (_, i) => i + 1);
}

export function hasArbiterRound(snapshot: ArenaRunSnapshot): boolean {
  return snapshot.run.findings.some((f) => f.rounds.some((r) => r.n === ARBITER_ROUND && r.log.length > 0))
    || (snapshot.run.config.rounds >= 2 && snapshot.run.reviewers.length >= 2);
}

/**
 * Whether the arbiter has ruled. Its ballots are the direct record; runs saved
 * before they were recorded still show the ruling as settled verdicts.
 */
export function arbiterRuled(snapshot: ArenaRunSnapshot): boolean {
  const findings = snapshot.run.findings;
  if (findings.some((f) => f.rounds.some((r) => r.n === ARBITER_ROUND && r.log.length > 0))) return true;
  return snapshot.run.config.rounds >= 2 && findings.some((f) => verdictKey(f.verdict) !== "pending");
}

export function ballotsAtRound(f: Pick<ArenaFinding, "rounds">, n: number): Ballot[] {
  return f.rounds.find((r) => r.n === n)?.log ?? [];
}

/** Each reviewer's vote in round `n`; a reviewer who cast none has no key. */
export function votesAtRound(f: Pick<ArenaFinding, "rounds">, n: number): Record<string, Vote> {
  const out: Record<string, Vote> = {};
  for (const b of ballotsAtRound(f, n)) out[b.reviewer] = b.vote;
  return out;
}

/** One reviewer's ballot per round, `undefined` where they cast none. */
export function ballotTrail(
  f: Pick<ArenaFinding, "rounds">,
  reviewerId: string,
  rounds: number[],
): (Ballot | undefined)[] {
  return rounds.map((n) => ballotsAtRound(f, n).find((b) => b.reviewer === reviewerId));
}

export function arbiterBallot(f: Pick<ArenaFinding, "rounds">): Ballot | undefined {
  return ballotsAtRound(f, ARBITER_ROUND)[0];
}

function dropMajority(ballots: Ballot[]): boolean {
  const voting = ballots.filter((b) => b.vote !== "abstain");
  if (voting.length === 0) return false;
  return voting.filter((b) => b.vote === "drop").length * 2 > voting.length;
}

/**
 * The round after which a finding was voted out, or null. The engine keeps
 * every finding on the ballot until the arbiter rules, so "out" is read from
 * the votes: the first round a majority of non-abstaining reviewers dropped
 * it. Only a finding that ended dropped (or never got a ruling) counts as out
 * early — one the arbiter kept stays in every column, so the bracket never
 * shows a finding leaving and coming back.
 */
export function outAfterRound(f: Pick<ArenaFinding, "rounds" | "verdict">, rounds: number[]): number | null {
  const v = verdictKey(f.verdict);
  if (v !== "dropped" && v !== "pending") return null;
  for (const n of rounds) {
    if (dropMajority(ballotsAtRound(f, n))) return n;
  }
  return null;
}

/** Whether a finding is still standing when round `n` starts. */
export function aliveInRound(f: Pick<ArenaFinding, "rounds" | "verdict">, n: number, rounds: number[]): boolean {
  const out = outAfterRound(f, rounds);
  return out === null || out >= n;
}

export function survivesArbiter(f: Pick<ArenaFinding, "verdict">): boolean {
  const v = verdictKey(f.verdict);
  return v === "kept" || v === "escalated";
}

export interface PageStats {
  proposed: number;
  kept: number;
  escalated: number;
  merged: number;
  dropped: number;
  pending: number;
}

/** Proposed counts findings, not producers: the strip reads as a funnel. */
export function pageStats(findings: Pick<ArenaFinding, "verdict">[]): PageStats {
  const s: PageStats = { proposed: findings.length, kept: 0, escalated: 0, merged: 0, dropped: 0, pending: 0 };
  for (const f of findings) s[verdictKey(f.verdict)] += 1;
  return s;
}

export interface FunnelStep {
  label: string;
  count: number;
  caption: string;
}

function listLabels(ids: string[], labels: Map<string, string>): string {
  const named = ids.map((id) => labels.get(id) ?? id);
  if (named.length <= 3) return named.join(", ");
  return `${named.slice(0, 3).join(", ")} +${named.length - 3}`;
}

export function funnelSteps(snapshot: ArenaRunSnapshot): FunnelStep[] {
  const findings = snapshot.run.findings;
  const labels = findingLabels(findings);
  const rounds = roundNumbers(snapshot);
  const ran = roundsRun(findings);
  const steps: FunnelStep[] = [{ label: "Proposed", count: findings.length, caption: "" }];
  for (const n of rounds) {
    if (n > ran) break;
    const out = findings.filter((f) => outAfterRound(f, rounds) === n).map((f) => f.id);
    const count = findings.filter((f) => {
      const o = outAfterRound(f, rounds);
      return o === null || o > n;
    }).length;
    let caption: string;
    if (out.length > 0) caption = `${listLabels(out, labels)} dropped by majority`;
    else if (n === 1) caption = "every proposal gets a round of votes";
    else caption = "nothing dropped";
    steps.push({ label: `After round ${n}`, count, caption });
  }
  if (arbiterRuled(snapshot)) {
    const s = pageStats(findings);
    const parts = [
      s.merged ? `${s.merged} merged` : "",
      s.escalated ? `${s.escalated} escalated` : "",
      s.dropped ? `${s.dropped} dropped` : "",
    ].filter(Boolean);
    steps.push({ label: "After arbiter", count: s.kept + s.escalated, caption: parts.join(", ") });
  }
  return steps;
}

export function isAccepted(run: Pick<ArenaRunSnapshot["run"], "accepted_finding_ids">, f: Pick<ArenaFinding, "id" | "accepted_at">): boolean {
  return Boolean(f.accepted_at) || (run.accepted_finding_ids ?? []).includes(f.id);
}

/**
 * What "Import into review" sends: kept and escalated survivors that are not
 * in the review yet, minus the ones left out. Merged findings stay out — their
 * claim already lives on the survivor they were folded into, which inherits
 * their producers. A human "Keep anyway" override turns a finding kept, so it
 * joins the import through its verdict rather than a side list.
 */
export function importableFindings(
  run: Pick<ArenaRunSnapshot["run"], "findings" | "accepted_finding_ids">,
  excluded: ReadonlySet<string> = new Set(),
): ArenaFinding[] {
  return run.findings.filter((f) => survivesArbiter(f) && !isAccepted(run, f) && !excluded.has(f.id));
}

export function importedCount(run: Pick<ArenaRunSnapshot["run"], "findings" | "accepted_finding_ids">): number {
  return run.findings.filter((f) => survivesArbiter(f) && isAccepted(run, f)).length;
}

/** Run #N by creation order, so the numbers stay put as newer runs land. */
export function runOrdinals(summaries: Pick<ArenaRunSummary, "id" | "created_at">[]): Map<string, number> {
  const sorted = [...summaries].sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
  return new Map(sorted.map((s, i) => [s.id, i + 1]));
}

export function runStatusLabel(status: RunStatus): string {
  if (typeof status === "object" && status !== null && "running" in status) return `running · round ${status.running.round}`;
  return status;
}

export function runDigest(snapshot: ArenaRunSnapshot): ArenaRunDigest {
  const s = pageStats(snapshot.run.findings);
  return {
    rounds: roundsRun(snapshot.run.findings),
    reviewers: snapshot.run.reviewers.length,
    survived: s.kept + s.escalated,
    total: s.proposed,
  };
}

export function digestLine(ordinal: number | undefined, d: ArenaRunDigest): string {
  const head = ordinal ? `Run #${ordinal} · ` : "";
  return `${head}${d.reviewers} reviewer${d.reviewers === 1 ? "" : "s"} · ${d.rounds} round${d.rounds === 1 ? "" : "s"} · ${d.survived} of ${d.total} findings survived`;
}

/** "2m 18s"; null when the run has not finished or a stamp does not parse. */
export function runDuration(createdAt: string, completedAt: string | undefined): string | null {
  if (!completedAt) return null;
  const ms = Date.parse(completedAt) - Date.parse(createdAt);
  if (!Number.isFinite(ms) || ms < 0) return null;
  const sec = Math.round(ms / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ${sec % 60}s`;
  return `${Math.floor(min / 60)}h ${min % 60}m`;
}

/** The arbiter column holds the ruling's headline; the detail holds the rest. */
export function firstSentence(text: string): string {
  const t = text.trim();
  const m = /^(.+?[.!?])(\s|$)/s.exec(t);
  return (m ? m[1] : t).trim();
}

export function latestSeverity(f: Pick<ArenaFinding, "severity_by_round">): ArenaSeverity {
  const rounds = Object.keys(f.severity_by_round).map(Number).sort((a, b) => b - a);
  const last = rounds[0];
  return last !== undefined ? f.severity_by_round[last] : "low";
}

/**
 * Reviewer colours come from theme tokens (ADR 0030), not the hex the engine
 * stamps on each reviewer, so they follow light and dark themes. A lens keeps
 * the colour of what it watches for; plain models take the categorical commit
 * series by position, which keeps neighbours distinct.
 */
const LENS_TOKENS: Record<string, string> = {
  general: "--color-accent",
  triage: "--color-info",
  professor: "--color-periwinkle",
  "expert:security": "--color-error",
  "expert:performance": "--color-periwinkle",
  "expert:reliability": "--color-success",
  "expert:testing": "--color-warning",
  "expert:api": "--color-info",
  "expert:patterns": "--color-emphasis",
  "expert:simplifying": "--color-action",
  "expert:mentorship": "--color-success",
};

export function lensColorVar(kind: string | undefined | null, index: number): string {
  const token = kind ? LENS_TOKENS[kind] ?? LENS_TOKENS[`expert:${kind}`] : undefined;
  return token ? `var(${token})` : commitColorVar(index);
}

export function reviewerColorVar(r: { id: string; agent_kind?: string }, index: number): string {
  return lensColorVar(r.agent_kind ?? (LENS_TOKENS[r.id] ? r.id : undefined), index);
}

export const ARBITER_COLOR = "var(--color-fg-3)";

/**
 * Launcher picks: a plain model reviews with the general prompt, a lens runs
 * as that agent on the shared agent model. Both land in one run, each
 * reviewer carrying its own `agent_kind`, so lenses vote on each other.
 */
export function modelPickKey(providerId: string, modelId: string): string {
  return `m:${providerId}::${modelId}`;
}

export function agentPickKey(kind: string): string {
  return `a:${kind}`;
}

export function picksToReviewers(keys: Iterable<string>, agentModel: ReviewerRef | null): ReviewerRef[] {
  const out: ReviewerRef[] = [];
  for (const key of keys) {
    if (key.startsWith("m:")) {
      const [provider_id = "", model_id = ""] = key.slice(2).split("::");
      out.push({ provider_id, model_id });
    } else if (key.startsWith("a:") && agentModel) {
      out.push({ provider_id: agentModel.provider_id, model_id: agentModel.model_id, agent_kind: key.slice(2) });
    }
  }
  return out;
}

export function reviewerToPickKey(r: ReviewerRef): string {
  return r.agent_kind ? agentPickKey(r.agent_kind) : modelPickKey(r.provider_id, r.model_id);
}

/** Whether Triage asked for this lens; triage names experts without the `expert:` prefix. */
export function triageRecommends(kind: string, experts: readonly string[]): boolean {
  return experts.some((e) => e === kind || `expert:${e}` === kind);
}

export type ReviewerRunState = "queued" | "running" | "done" | "failed";

/**
 * How far one reviewer is through the run, 0–100. Progress only reports who
 * is thinking and who has finished in the current round, so a reviewer's bar
 * fills a round's share at a time and sits half way through its share while
 * it thinks. Once the arbiter is ruling every reviewer is finished.
 */
export function reviewerProgress(
  state: ReviewerRunState,
  round: number,
  totalRounds: number,
  arbiterPhase: boolean,
): number {
  if (arbiterPhase) return 100;
  const total = Math.max(1, totalRounds);
  const r = Math.min(Math.max(1, round), total);
  const share = 100 / total;
  const base = (r - 1) * share;
  if (state === "done") return Math.round(base + share);
  if (state === "running") return Math.round(base + share / 2);
  return Math.round(base);
}

/** Banner for a run whose result never reaches the review. */
export function endedRunNotice(
  snapshot: ArenaRunSnapshot,
  ordinal: number | undefined,
): { title: string; body: string } | null {
  const status = snapshot.run.status;
  if (status !== "cancelled" && status !== "failed") return null;
  const name = ordinal ? `Run #${ordinal}` : "This run";
  const k = roundsRun(snapshot.run.findings);
  const when = k > 0 ? ` after round ${k}` : " before round 1 finished";
  const n = snapshot.run.reviewers.length;
  const p = snapshot.run.findings.length;
  const title = status === "cancelled" ? `${name} was cancelled${when}` : `${name} failed${when}`;
  const body = `${n} reviewer${n === 1 ? "" : "s"}, ${p} proposed finding${p === 1 ? "" : "s"}, no arbiter pass. Nothing from a ${status} run reaches the review.`;
  return { title, body };
}
