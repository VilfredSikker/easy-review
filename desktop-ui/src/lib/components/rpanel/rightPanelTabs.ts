/**
 * The panel beside Tour and Diff answers what did the review find, which files
 * need a close read, what has been said, and how the change hangs together —
 * so those are its tabs. Diagrams are on the Overview as well: reading a flow
 * diagram is most useful next to the code it draws. Branch and Triage held
 * things that belong on the Overview only; the names stay so a stored or
 * requested one can be sent there instead of to an empty panel.
 */
export type PrimaryRightPanelTab = "findings" | "risks" | "comments" | "diagrams";
export type SecondaryRightPanelTab = "branch" | "triage";
export type RightPanelTab = PrimaryRightPanelTab | SecondaryRightPanelTab;

/** Names earlier panels used that no longer name a tab. `context` was the
 *  Diagrams tab's first name. */
export type LegacyRightPanelTab = "review" | "notes" | "context";

export const PRIMARY_TABS: readonly PrimaryRightPanelTab[] = ["findings", "risks", "comments", "diagrams"];
export const SECONDARY_TABS: readonly SecondaryRightPanelTab[] = ["branch", "triage"];

export const SECONDARY_TAB_LABELS: Record<SecondaryRightPanelTab, string> = {
  branch: "Branch & PR",
  triage: "Triage & checklist",
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
  if (raw === "context") return "diagrams";
  if (raw && ALL_TABS.includes(raw)) return raw as RightPanelTab;
  return "findings";
}

/**
 * The value under the four-tab key. "branch" was that panel's default, so a
 * stored "branch" mostly records that nobody chose; it opens on Findings like a
 * fresh install rather than on a tab that has moved to the Overview.
 */
export function migrateLegacyStoredTab(raw: string | null | undefined): RightPanelTab {
  if (raw === "branch") return "findings";
  return migrateRightPanelTab(raw);
}
