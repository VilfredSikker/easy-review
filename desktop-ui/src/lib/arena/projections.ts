import type {
  ArenaFinding,
  ArenaRun,
  ArenaRunSnapshot,
  FunnelStage,
  FunnelStages,
  MatrixRow,
  Verdict,
  Vote,
} from "$lib/types/arena";

export function buildMatrix(findings: ArenaFinding[]): MatrixRow[] {
  return findings.map((f) => {
    const latest_vote: Record<string, Vote> = {};
    for (const round of f.rounds) {
      for (const ballot of round.log) {
        latest_vote[ballot.reviewer] = ballot.vote;
      }
    }
    return {
      finding_id: f.id,
      latest_vote,
      verdict: f.verdict,
      confidence: f.confidence,
    };
  });
}

export function buildFunnel(findings: ArenaFinding[]): FunnelStages {
  const proposed = findings.length;
  let cross_checked = 0;
  let resolved = 0;
  let finalCount = 0;
  const exited_at: Record<string, FunnelStage> = {};

  for (const f of findings) {
    const hasRound2 = f.rounds.some((r) => r.n >= 2 && r.log.length > 0);
    const hasRound3 = f.rounds.some((r) => r.n >= 3 && r.log.length > 0);
    if (hasRound2) cross_checked += 1;
    if (hasRound3 || !isPendingVerdict(f.verdict)) resolved += 1;

    if (f.verdict === "kept" || f.verdict === "escalated") {
      finalCount += 1;
    } else if (typeof f.verdict === "object" && "merged" in f.verdict) {
      finalCount += 1;
      exited_at[f.id] = "resolved";
    } else if (f.verdict === "dropped") {
      exited_at[f.id] = "cross_checked";
    }
  }

  return {
    counts: { proposed, cross_checked, resolved, final: finalCount },
    exited_at,
  };
}

function isPendingVerdict(v: Verdict): boolean {
  return v === "pending";
}

export function buildSnapshot(run: ArenaRun): ArenaRunSnapshot {
  return {
    run,
    matrix: buildMatrix(run.findings),
    funnel: buildFunnel(run.findings),
  };
}

export function arenaStats(findings: ArenaFinding[]) {
  const verdicts = { kept: 0, escalated: 0, merged: 0, dropped: 0 };
  let proposed = 0;
  for (const f of findings) {
    proposed += f.raised_by.length;
    if (f.verdict === "kept") verdicts.kept += 1;
    else if (f.verdict === "escalated") verdicts.escalated += 1;
    else if (typeof f.verdict === "object") verdicts.merged += 1;
    else if (f.verdict === "dropped") verdicts.dropped += 1;
  }
  return {
    proposed,
    total: findings.length,
    finalCount: verdicts.kept + verdicts.escalated + verdicts.merged,
    verdicts,
  };
}

/** The default flow exposes only kept or escalated survivors. Manual arena
 * import keeps its broader historical set, including merged findings. Seeded
 * verification exposes every verdict because it is a read-only result log. */
export function finalTruthFindings<T extends Pick<ArenaFinding, "id" | "verdict" | "accepted_at">>(
  findings: T[],
  flowAcceptance = false,
  verificationMode = false,
): T[] {
  if (verificationMode) return findings;
  return findings.filter((finding) => {
    if (flowAcceptance) return finding.verdict === "kept" || finding.verdict === "escalated";
    return finding.verdict !== "dropped" && finding.verdict !== "pending";
  });
}

/** Unaccepted items that the current result origin permits importing. */
export function pendingAcceptanceFindings<T extends Pick<ArenaFinding, "id" | "verdict" | "accepted_at">>(
  findings: T[],
  acceptedIds: string[] = [],
  flowAcceptance = false,
): T[] {
  const accepted = new Set(acceptedIds);
  return finalTruthFindings(findings, flowAcceptance).filter((finding) => {
    if (finding.accepted_at || accepted.has(finding.id)) return false;
    return true;
  });
}
