/**
 * The settings payload's risk-rule fields (ADR 0048) and the `risk.rule` patch
 * this page writes. Kept here until the shared types mirror them.
 */

export type RiskTier = "high" | "med" | "low" | "ai";
export type RiskRuleSource = "project" | "user";

export const RISK_TIERS: readonly RiskTier[] = ["high", "med", "low", "ai"];

export interface RiskRuleRow {
  pattern: string;
  /** The canonical tier, or the text as written when it does not read. */
  tier: string;
  readable: boolean;
  label: string | null;
  source: RiskRuleSource;
  /** Changed files of the active tab this rule claims. Zero is an idle rule. */
  fileCount: number;
}

export interface RiskFileRow {
  path: string;
  tier: RiskTier;
  rule: { key: string; label: string | null; source: RiskRuleSource } | null;
}

/** The risk fields of the settings snapshot; absent on an engine that predates them. */
export interface RiskSettingsPayload {
  riskRepo?: string;
  riskRules?: RiskRuleRow[];
  riskDefault?: RiskTier;
  riskFiles?: RiskFileRow[];
}

export const RISK_RULE_PATCH_KEY = "risk.rule";

export function tierLabel(tier: string): string {
  return tier === "ai" ? "AI" : tier;
}

export function isRiskTier(tier: string): tier is RiskTier {
  return (RISK_TIERS as readonly string[]).includes(tier);
}

export interface RiskRuleDraft {
  source: RiskRuleSource;
  pattern: string;
  tier: RiskTier;
  label: string;
}

/**
 * Why a draft cannot be sent, or null. The engine refuses an invalid edit
 * without saying so on the patch wire, so the page checks what it can first;
 * a glob that does not compile is caught after the write, by the rule not
 * appearing.
 */
export function draftProblem(draft: RiskRuleDraft, repo: string, rules: RiskRuleRow[], replaces?: string): string | null {
  const pattern = draft.pattern.trim();
  if (!pattern) return "A rule needs a pattern.";
  if (!isRiskTier(draft.tier)) return "The tier must be high, med, low or AI.";
  if (draft.source === "project" && !repo.trim()) return "Open a repo to add a project rule.";
  const clash = rules.some((r) => r.source === draft.source && r.pattern === pattern && r.pattern !== replaces);
  if (clash) return `${pattern} already has a ${draft.source === "project" ? "project" : "user"} rule.`;
  return null;
}

/** The `risk.rule` patch value for setting (or renaming, with `replaces`) a rule. */
export function setRuleEdit(draft: RiskRuleDraft, repo: string, replaces?: string): string {
  const label = draft.label.trim();
  return JSON.stringify({
    source: draft.source,
    repo,
    pattern: draft.pattern.trim(),
    tier: draft.tier,
    label: label || null,
    replaces: replaces && replaces !== draft.pattern.trim() ? replaces : null,
  });
}

/** The `risk.rule` patch value that removes a rule: an edit with no tier. */
export function removeRuleEdit(rule: Pick<RiskRuleRow, "source" | "pattern">, repo: string): string {
  return JSON.stringify({ source: rule.source, repo, pattern: rule.pattern });
}

/** Whether the snapshot after a write holds the rule the draft asked for. */
export function ruleLanded(rules: RiskRuleRow[], draft: RiskRuleDraft): boolean {
  const pattern = draft.pattern.trim();
  return rules.some((r) => r.source === draft.source && r.pattern === pattern && r.tier === draft.tier);
}

/** Changed-file rows past this are counted, not listed, so a large diff
 * cannot push the rules table off the page. */
export const RISK_FILE_LIMIT = 40;

export function fileWindow<T>(files: T[], limit: number = RISK_FILE_LIMIT): { shown: T[]; hidden: number } {
  if (files.length <= limit) return { shown: files, hidden: 0 };
  return { shown: files.slice(0, limit), hidden: files.length - limit };
}

/** What the file list names as the source of a file's tier. A file no rule
 * claimed needs words of its own: an empty cell reads as missing data. */
export function claimedBy(file: RiskFileRow): string {
  return file.rule?.key ?? "no rule";
}
