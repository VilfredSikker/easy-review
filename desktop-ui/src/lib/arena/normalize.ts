import type { ArenaRunSnapshot, ArenaSeverity } from "$lib/types/arena";

function normSeverity(raw: string): ArenaSeverity {
  if (raw === "medium") return "med";
  if (raw === "high" || raw === "med" || raw === "low" || raw === "info") return raw;
  return "info";
}

/** Map backend RiskLevel JSON (`medium`) to UI contract (`med`). */
export function normalizeArenaSnapshot(snap: ArenaRunSnapshot): ArenaRunSnapshot {
  const findings = snap.run.findings.map((f) => ({
    ...f,
    severity_by_round: Object.fromEntries(
      Object.entries(f.severity_by_round).map(([round, sev]) => [
        Number(round),
        normSeverity(String(sev)),
      ]),
    ) as Record<number, ArenaSeverity>,
  }));
  return {
    ...snap,
    run: { ...snap.run, findings },
  };
}
