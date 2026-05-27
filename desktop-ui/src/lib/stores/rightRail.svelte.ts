// Single source of truth for the right panel's collapsed/expanded state.
// Both App.svelte (consumer) and keyboard.ts (toggle) reach for this —
// keeps the prop-passing chain short and `]` key functional without needing
// App.svelte in the loop.
//
// `collapsed` is persisted to localStorage so the state survives app restarts.

const STORAGE_KEY = "rightPanelCollapsed";

function loadInitial(): boolean {
  if (typeof localStorage === "undefined") return false;
  try {
    return localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

function persist(v: boolean) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, String(v));
  } catch {
    /* ignore quota / privacy-mode errors */
  }
}

function createRightRailStore() {
  let collapsed = $state(loadInitial());

  return {
    get collapsed() {
      return collapsed;
    },
    set(v: boolean) {
      collapsed = v;
      persist(v);
    },
    toggle() {
      collapsed = !collapsed;
      persist(collapsed);
    },
    expand() {
      collapsed = false;
      persist(false);
    },
  };
}

export const rightRail = createRightRailStore();
