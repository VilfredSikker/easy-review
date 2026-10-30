/**
 * The panel beside Tour and Diff answers three questions — what did the review
 * find, which files need a close read, what has been said — so those are its
 * tabs. Branch, Triage and Context held things that belong on the Overview;
 * until that page carries them they stay reachable behind the panel's "More"
 * menu rather than being dropped.
 */
export type PrimaryRightPanelTab = "findings" | "risks" | "comments";
export type SecondaryRightPanelTab = "branch" | "triage" | "context";
export type RightPanelTab = PrimaryRightPanelTab | SecondaryRightPanelTab;

/** Names the four-tab panel used that no longer name a tab. */
export type LegacyRightPanelTab = "review" | "notes";

export const PRIMARY_TABS: readonly PrimaryRightPanelTab[] = ["findings", "risks", "comments"];
export const SECONDARY_TABS: readonly SecondaryRightPanelTab[] = ["branch", "triage", "context"];

export const SECONDARY_TAB_LABELS: Record<SecondaryRightPanelTab, string> = {
  branch: "Branch & PR",
  triage: "Triage & checklist",
  context: "Diagrams",
};

/** A new key, so a stored four-tab value is read through the migration once
 *  instead of being reinterpreted on every load. */
export const RIGHT_PANEL_TAB_KEY = "er.rightPanel.tab";
export const LEGACY_RIGHT_PANEL_TAB_KEY = "rightPanelActiveTab";

const ALL_TABS: readonly string[] = [...PRIMARY_TABS, ...SECONDARY_TABS];

export function isSecondaryTab(tab: RightPanelTab): tab is SecondaryRightPanelTab {
  return (SECONDARY_TABS as readonly string[]).includes(tab);
}

/** Any stored or requested name to a tab that exists. Review held the findings;
 *  Notes held questions and notes, which the Comments tab now lists. */
export function migrateRightPanelTab(raw: string | null | undefined): RightPanelTab {
  if (raw === "review") return "findings";
  if (raw === "notes") return "comments";
  if (raw && ALL_TABS.includes(raw)) return raw as RightPanelTab;
  return "findings";
}

/**
 * The value under the four-tab key. "branch" was that panel's default, so a
 * stored "branch" mostly records that nobody chose; it opens on Findings like a
 * fresh install rather than on a tab that is now tucked behind "More".
 */
export function migrateLegacyStoredTab(raw: string | null | undefined): RightPanelTab {
  if (raw === "branch") return "findings";
  return migrateRightPanelTab(raw);
}
