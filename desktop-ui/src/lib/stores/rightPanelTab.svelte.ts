// Single source of truth for the right panel's active tab. Lives in a store
// (not component state) so the command palette and the collapsed rail can
// switch tabs even while the panel is already expanded — a plain localStorage
// write wouldn't reach a mounted RightPanel.

import {
  migrateLegacyStoredTab,
  migrateRightPanelTab,
  RIGHT_PANEL_TAB_KEY,
  LEGACY_RIGHT_PANEL_TAB_KEY,
  type LegacyRightPanelTab,
  type RightPanelTab,
} from "$lib/components/rpanel/rightPanelTabs";

export type { RightPanelTab } from "$lib/components/rpanel/rightPanelTabs";

function loadInitial(): RightPanelTab {
  if (typeof localStorage === "undefined") return "findings";
  try {
    const current = localStorage.getItem(RIGHT_PANEL_TAB_KEY);
    if (current !== null) return migrateRightPanelTab(current);
    // One-time read of the four-tab key, so a reader who left the panel on
    // Notes lands on Comments rather than on the default.
    const legacy = localStorage.getItem(LEGACY_RIGHT_PANEL_TAB_KEY);
    const tab = migrateLegacyStoredTab(legacy);
    localStorage.setItem(RIGHT_PANEL_TAB_KEY, tab);
    localStorage.removeItem(LEGACY_RIGHT_PANEL_TAB_KEY);
    return tab;
  } catch {
    return "findings";
  }
}

function persist(v: RightPanelTab) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(RIGHT_PANEL_TAB_KEY, v);
  } catch {
    /* ignore quota / privacy-mode errors */
  }
}

function createRightPanelTabStore() {
  let active = $state<RightPanelTab>(loadInitial());

  return {
    get active() {
      return active;
    },
    // Takes the retired names too: callers written against the four-tab panel
    // keep landing on the tab that now holds what they meant.
    set(v: RightPanelTab | LegacyRightPanelTab) {
      active = migrateRightPanelTab(v);
      persist(active);
    },
  };
}

export const rightPanelTab = createRightPanelTabStore();
