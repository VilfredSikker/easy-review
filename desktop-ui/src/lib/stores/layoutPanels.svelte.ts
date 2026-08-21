// Left sidebar and file-tree visibility. Same idea as `rightRail`: the UI
// paints from this store, not from `snapshot.panels`. Persistence uses
// *Collapsed keys (true = hidden) so they match `rightPanelCollapsed`.

import { visibleFromCollapsedItem } from "./layoutPanelVisibility";

export const LEFT_COLLAPSED_KEY = "leftPanelCollapsed";
export const TREE_COLLAPSED_KEY = "fileTreeCollapsed";

function loadVisible(key: string): boolean {
  if (typeof localStorage === "undefined") return true;
  try {
    return visibleFromCollapsedItem(localStorage.getItem(key));
  } catch {
    return true;
  }
}

function persistCollapsed(key: string, visible: boolean) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(key, String(!visible));
  } catch {
    /* ignore quota / privacy-mode errors */
  }
}

function createLayoutPanelsStore() {
  let left = $state(loadVisible(LEFT_COLLAPSED_KEY));
  let tree = $state(loadVisible(TREE_COLLAPSED_KEY));

  return {
    get left() {
      return left;
    },
    get tree() {
      return tree;
    },
    toggle(panel: "left" | "tree") {
      if (panel === "left") {
        left = !left;
        persistCollapsed(LEFT_COLLAPSED_KEY, left);
      } else {
        tree = !tree;
        persistCollapsed(TREE_COLLAPSED_KEY, tree);
      }
    },
  };
}

export const layoutPanels = createLayoutPanelsStore();
