import { COMMENTS_KIND_KEY, loadKindFilter, type KindFilter } from "./commentsPanel";
import type { SeverityFilter } from "./findingsPanel";

function initialKind(): KindFilter {
  if (typeof localStorage === "undefined") return "all";
  try {
    return loadKindFilter(localStorage);
  } catch {
    return "all";
  }
}

// Outlives the panel, which remounts on every view switch. The severity tile is
// session-only for the reason the gate is (findingsVisibility): a remembered
// filter would hide findings in a review it was never set for. The kind filter
// is a reading preference, so it persists like the Notes sub-tab it replaces.
class RightPanelFilters {
  severity = $state<SeverityFilter>("all");
  kind = $state<KindFilter>(initialKind());

  setKind(kind: KindFilter) {
    this.kind = kind;
    try {
      localStorage.setItem(COMMENTS_KIND_KEY, kind);
    } catch {
      /* ignore quota / privacy-mode errors */
    }
  }
}

export const rightPanelFilters = new RightPanelFilters();
