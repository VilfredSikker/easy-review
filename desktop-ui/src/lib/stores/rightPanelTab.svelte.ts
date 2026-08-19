// Single source of truth for the right panel's active tab. Lives in a store
// (not component state) so the command palette and the collapsed rail can
// switch tabs even while the panel is already expanded — a plain localStorage
// write wouldn't reach a mounted RightPanel.

const STORAGE_KEY = "rightPanelActiveTab";

export type RightPanelTab = "branch" | "review" | "notes" | "context";

const VALID: readonly string[] = ["branch", "review", "notes", "context"];

function loadInitial(): RightPanelTab {
  if (typeof localStorage === "undefined") return "branch";
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw && VALID.includes(raw)) return raw as RightPanelTab;
  } catch {
    /* ignore */
  }
  return "branch";
}

function persist(v: RightPanelTab) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, v);
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
    set(v: RightPanelTab) {
      active = v;
      persist(v);
    },
  };
}

export const rightPanelTab = createRightPanelTabStore();
