import type { FileRiskSnapshot } from "$lib/types";

/** CONTEXT.md **Tier**. `ai` means no rule fixed one. */
export type RiskTier = "high" | "med" | "low" | "ai";
export type RiskRuleSource = "project" | "user";

/**
 * The risk-rule fields of `get_config_hub`'s settings snapshot
 * (`DesktopSettingsSnapshot` in er-engine's `config/desktop_settings.rs`,
 * camelCase over IPC). `types.ts` does not mirror them yet, so this panel
 * declares the slice it reads; keep it in step with that struct.
 */
export interface RiskRuleWire {
  pattern: string;
  /** Canonical tier, or the text as written when it does not read. */
  tier: string;
  readable: boolean;
  label: string | null;
  source: RiskRuleSource;
  fileCount: number;
}

export interface MatchedRiskRuleWire {
  key: string;
  label: string | null;
  source: RiskRuleSource;
}

export interface RiskFileWire {
  path: string;
  tier: RiskTier;
  rule: MatchedRiskRuleWire | null;
}

export interface RiskSettingsWire {
  riskRules: RiskRuleWire[];
  riskDefault: RiskTier;
  /** Empty when nothing is declared: every path then reads as `ai`. */
  riskFiles: RiskFileWire[];
}

export interface RiskRow {
  path: string;
  /** The tier the rules gave the file. */
  tier: RiskTier;
  /**
   * The tier the row is grouped under: the declared one, or for an `ai` file
   * the review's rating of it. Still `ai` when no review has rated the file —
   * ADR 0048 shows it unrated rather than defaulting it to `low`.
   */
  shown: RiskTier;
  rule: MatchedRiskRuleWire | null;
  /** The review's reason, kept only where the review set the tier. */
  aiReason: string | null;
  reviewed: boolean;
}

export const TIER_ORDER: readonly RiskTier[] = ["high", "med", "low", "ai"];
const TIER_RANK: Record<RiskTier, number> = { high: 0, med: 1, low: 2, ai: 3 };

interface FileLike {
  path: string;
  reviewed: boolean;
  /** The review's rating, when the snapshot carries one on the file. */
  risk?: FileRiskSnapshot["risk"] | null;
}

/**
 * One row per changed file. A declared tier wins; the AI review only answers
 * for files the rules left to it, which is what a tier of `ai` means.
 */
export function riskRows(
  files: readonly FileLike[],
  resolved: readonly RiskFileWire[],
  aiRisks: readonly FileRiskSnapshot[],
): RiskRow[] {
  const byPath = new Map(resolved.map((r) => [r.path, r]));
  const aiByPath = new Map(aiRisks.map((r) => [r.path, r]));
  const rows = files.map((file): RiskRow => {
    const declared = byPath.get(file.path);
    const tier = declared?.tier ?? "ai";
    const ai = aiByPath.get(file.path);
    const aiRating = ai?.risk ?? file.risk ?? null;
    const aiSets = tier === "ai" && aiRating !== null;
    return {
      path: file.path,
      tier,
      shown: aiSets ? aiRating : tier,
      rule: declared?.rule ?? null,
      aiReason: aiSets ? (ai?.risk_reason || null) : null,
      reviewed: file.reviewed,
    };
  });
  return rows.sort((a, b) => TIER_RANK[a.shown] - TIER_RANK[b.shown] || a.path.localeCompare(b.path));
}

export interface RiskGroup {
  tier: RiskTier;
  rows: RiskRow[];
}

export function groupRiskRows(rows: readonly RiskRow[]): RiskGroup[] {
  return TIER_ORDER.map((tier) => ({ tier, rows: rows.filter((r) => r.shown === tier) })).filter(
    (g) => g.rows.length > 0,
  );
}

export function tierCounts(rows: readonly RiskRow[]): Record<RiskTier, number> {
  const counts: Record<RiskTier, number> = { high: 0, med: 0, low: 0, ai: 0 };
  for (const r of rows) counts[r.shown]++;
  return counts;
}

/** High-risk files still to read — the one number the tab exists to surface. */
export function unreviewedHigh(rows: readonly RiskRow[]): number {
  return rows.filter((r) => r.shown === "high" && !r.reviewed).length;
}

/** Group header text. `ai` with no rating is the review's job, not a level. */
export function tierHeading(tier: RiskTier): string {
  return tier === "ai" ? "AI · not rated yet" : tier;
}

export function sourceLabel(source: RiskRuleSource): string {
  return source === "project" ? "project" : "yours";
}
