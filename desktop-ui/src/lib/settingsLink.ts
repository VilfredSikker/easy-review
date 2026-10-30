import type { SettingsTab } from "./types";

/** A Settings section other surfaces link to by name. */
export type SettingsSectionId = "risk-rules" | "concurrency" | "uninstall";

/** Where an open of Settings lands: a tab, and optionally a section on it. */
export interface SettingsTarget {
  tab: SettingsTab;
  section: SettingsSectionId | null;
}

// The section decides the tab, so a caller names only what it means ("Risk
// rules") and cannot pair it with the wrong tab.
const SECTION_TAB: Record<SettingsSectionId, SettingsTab> = {
  "risk-rules": "review",
  concurrency: "ai",
  uninstall: "general",
};

const TABS: readonly SettingsTab[] = ["general", "ai", "review", "projects", "terminal"];

export function isSettingsSection(to: string): to is SettingsSectionId {
  return Object.hasOwn(SECTION_TAB, to);
}

/** Resolve a link to a tab or a section; null for a plain open, which lands on Settings' first tab. */
export function resolveSettingsTarget(to: SettingsTab | SettingsSectionId | null | undefined): SettingsTarget | null {
  if (!to) return null;
  if (isSettingsSection(to)) return { tab: SECTION_TAB[to], section: to };
  return TABS.includes(to) ? { tab: to, section: null } : null;
}

/** The DOM id a linked section renders under, so the page can scroll it into view. */
export function settingsSectionDomId(section: SettingsSectionId): string {
  return `settings-${section}`;
}
