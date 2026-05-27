import type { ArenaSeverity, Verdict, Vote } from "$lib/types/arena";

export function voteGlyph(vote: Vote): string {
  switch (vote) {
    case "keep":
      return "✓";
    case "drop":
      return "✕";
    case "merge":
      return "⊕";
    case "escalate":
      return "↑";
    case "lower":
      return "↓";
    case "flag":
      return "⚑";
    case "abstain":
      return "·";
    case "propose":
      return "+";
    default:
      return "?";
  }
}

export function verdictLabel(verdict: Verdict): string {
  if (verdict === "kept") return "kept";
  if (verdict === "escalated") return "escalated";
  if (verdict === "dropped") return "dropped";
  if (verdict === "pending") return "pending";
  if (typeof verdict === "object" && "merged" in verdict) return "merged";
  return "unknown";
}

export function severityTone(sev: ArenaSeverity): string {
  switch (sev) {
    case "high":
      return "text-[var(--arena-err)]";
    case "med":
      return "text-[var(--arena-warn)]";
    case "low":
      return "text-[var(--arena-info)]";
    default:
      return "text-[var(--arena-fg-muted)]";
  }
}

export function verdictPillClass(verdict: Verdict): string {
  const base = "arena-pill ";
  if (verdict === "kept") return base + "arena-pill--kept";
  if (verdict === "escalated") return base + "arena-pill--err";
  if (verdict === "dropped") return base + "arena-pill--muted line-through";
  if (typeof verdict === "object") return base + "arena-pill--merged";
  return base + "arena-pill--pending";
}

export function basename(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}
