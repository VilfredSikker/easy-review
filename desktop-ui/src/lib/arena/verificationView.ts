import { arbiterBallot, roundsRun, verdictKey, type VerdictKey } from "$lib/arena/arenaView";
import type { ArenaFinding, ArenaRunSnapshot, ArenaSeverity, Reviewer, RunStatus } from "$lib/types/arena";
import type { HunkSnapshot } from "$lib/types";

/** The stat tiles double as the list filter; "all" clears it. */
export type VerificationFilter = "all" | VerdictKey;

/**
 * List order puts what needs a decision first: escalations the arbiter raised,
 * then what it kept, then what it folded away or threw out.
 */
const GROUP_ORDER: VerdictKey[] = ["escalated", "kept", "merged", "dropped", "pending"];

export interface VerificationGroup<T> {
  key: VerdictKey;
  findings: T[];
}

export function matchesFilter(f: Pick<ArenaFinding, "verdict">, filter: VerificationFilter): boolean {
  return filter === "all" || verdictKey(f.verdict) === filter;
}

export function verificationGroups<T extends Pick<ArenaFinding, "verdict">>(
  findings: T[],
  filter: VerificationFilter,
): VerificationGroup<T>[] {
  return GROUP_ORDER.map((key) => ({
    key,
    findings: findings.filter((f) => verdictKey(f.verdict) === key && matchesFilter(f, filter)),
  })).filter((g) => g.findings.length > 0);
}

/** The ids in the order the list shows them, which is what j/k walks. */
export function visibleOrder<T extends Pick<ArenaFinding, "id" | "verdict">>(
  findings: T[],
  filter: VerificationFilter,
): string[] {
  return verificationGroups(findings, filter).flatMap((g) => g.findings.map((f) => f.id));
}

/** j/k stop at the ends rather than wrap, so holding a key never loops past the last finding. */
export function stepSelection(order: string[], current: string | null, delta: 1 | -1): string | null {
  if (order.length === 0) return null;
  const i = current === null ? -1 : order.indexOf(current);
  if (i === -1) return order[0] ?? null;
  return order[Math.max(0, Math.min(order.length - 1, i + delta))] ?? null;
}

/** "low → med" when the severity changed between the first and the last recorded round. */
export function regrade(f: Pick<ArenaFinding, "severity_by_round">): string | null {
  const rounds = Object.keys(f.severity_by_round).map(Number).sort((a, b) => a - b);
  if (rounds.length < 2) return null;
  const first = f.severity_by_round[rounds[0] ?? 0];
  const last = f.severity_by_round[rounds.at(-1) ?? 0];
  return first && last && first !== last ? `${first} → ${last}` : null;
}

/** "General", "General + Security", or "General +2" for a finding several lenses raised. */
export function lensNames(f: Pick<ArenaFinding, "raised_by">, reviewers: Pick<Reviewer, "id" | "name">[]): string {
  const names = f.raised_by.map((id) => reviewers.find((r) => r.id === id)?.name ?? id);
  if (names.length === 0) return "Finder";
  if (names.length <= 2) return names.join(" + ");
  return `${names[0]} +${names.length - 1}`;
}

const WHY_BUDGET = 220;

/**
 * The engine records one ruling per finding, so the comparison card shows its
 * opening and "Read full reasoning" continues from there instead of repeating
 * it. The arbiter's ballot note is a separate record; it joins the remainder
 * only when it says something the rationale does not.
 */
export function splitReasoning(
  f: Pick<ArenaFinding, "rationale" | "rounds">,
): { why: string; full: string } {
  const rationale = f.rationale.trim();
  const note = arbiterBallot(f)?.note?.trim() ?? "";
  const text = rationale || note;
  const extra = rationale && note && !rationale.includes(note) ? note : "";
  if (text.length <= WHY_BUDGET) return { why: text, full: extra };
  const sentences = text.match(/[^.!?]+(?:[.!?]+|$)\s*/g) ?? [text];
  let why = "";
  for (const s of sentences) {
    if (why && why.length + s.length > WHY_BUDGET) break;
    why += s;
  }
  const rest = text.slice(why.length).trim();
  return { why: why.trim(), full: [rest, extra].filter(Boolean).join("\n\n") };
}

export interface ExcerptLine {
  num: number;
  text: string;
}

/**
 * Source lines around the finding, read from the diff already loaded for the
 * tab. Evidence ranges win over the finding's own line; a line outside every
 * loaded hunk has no text to show, and the excerpt is left out rather than
 * fetched, because the arena records lines, not their contents.
 */
export function evidenceExcerpt(
  hunks: Pick<HunkSnapshot, "lines">[],
  f: Pick<ArenaFinding, "file" | "line" | "evidence">,
  maxLines = 8,
): ExcerptLine[] {
  const ranges: (readonly [number, number])[] = [];
  for (const e of f.evidence ?? []) {
    if (e.file !== f.file || !e.line_start) continue;
    ranges.push([e.line_start, Math.max(e.line_start, e.line_end ?? e.line_start)]);
  }
  if (ranges.length === 0 && f.line) ranges.push([f.line - 1, f.line + 1]);
  if (ranges.length === 0) return [];
  const out: ExcerptLine[] = [];
  const seen = new Set<number>();
  for (const h of hunks) {
    for (const l of h.lines) {
      const n = l.new_num;
      if (n === null || l.kind === "del" || seen.has(n)) continue;
      if (!ranges.some(([a, b]) => n >= a && n <= b)) continue;
      seen.add(n);
      out.push({ num: n, text: l.text });
    }
  }
  return out.sort((a, b) => a.num - b.num).slice(0, maxLines);
}

export type NoteTone = "ok" | "warn" | "info" | "err";

function runActive(status: RunStatus): boolean {
  return status === "queued" || (typeof status === "object" && status !== null && "running" in status);
}

/**
 * What happens to the results. A seeded verification applies itself to the
 * review; a flow's Deep pass waits for an explicit accept (ADR 0024).
 */
export function verificationNote(opts: {
  seeded: boolean;
  status: RunStatus;
  readOnlyFlow: boolean;
  awaiting: number;
  seededText: string;
}): { tone: NoteTone; text: string } {
  if (opts.seeded) {
    if (opts.status === "complete") return { tone: "ok", text: `✓ ${opts.seededText}` };
    if (opts.status === "failed") return { tone: "err", text: opts.seededText };
    return { tone: "info", text: opts.seededText };
  }
  if (runActive(opts.status)) {
    return { tone: "info", text: "Verification is running. Results can be accepted when it completes." };
  }
  if (opts.readOnlyFlow) {
    return { tone: "info", text: "This historical flow result is read only. Start a new review to accept findings." };
  }
  if (opts.awaiting > 0) {
    return { tone: "warn", text: "Deep results wait for you. Nothing reaches the review until you accept." };
  }
  return {
    tone: "ok",
    text: "✓ Verification complete. Accepted findings are in the review; dropped ones stay here for reference.",
  };
}

/** The header's run line: what was checked, by what, and against which diff. */
export function verificationMeta(snapshot: ArenaRunSnapshot): string {
  const run = snapshot.run;
  const n = run.findings.length;
  const lenses = run.reviewers.length;
  const ran = roundsRun(run.findings);
  const rounds = ran > 0 ? ran : run.config.rounds;
  const verifier = snapshot.arbiter?.label ?? run.config.arbiter.model_id;
  return [
    `${run.config.run_kind === "seeded" ? "Verification run" : "Review run"} ${run.id}`,
    `${n} finding${n === 1 ? "" : "s"} from ${lenses} lens${lenses === 1 ? "" : "es"}`,
    verifier ? `verifier ${verifier}` : "",
    rounds > 0 ? `${rounds} round${rounds === 1 ? "" : "s"}` : "",
    run.diff_hash ? `built from ${run.diff_hash.slice(0, 7)}` : "",
  ]
    .filter(Boolean)
    .join(" · ");
}

/** The severity dot has three tiers; "info" sits with low rather than getting a fourth colour. */
export function severityKey(sev: ArenaSeverity): "high" | "med" | "low" {
  return sev === "high" || sev === "med" ? sev : "low";
}
