// Single source of truth for the bottom terminal drawer's open/closed state.
// Both App.svelte (consumer) and keyboard.ts / CommandPalette.svelte (toggles)
// reach for this — keeps the prop-passing chain short.
//
// `open` is persisted to localStorage so the drawer stays open across app
// restarts. We deliberately do NOT persist scrollback — closing still kills
// the PTY (see Terminal.svelte teardown).

const STORAGE_KEY = "terminalOpen";

function loadInitial(): boolean {
  if (typeof localStorage === "undefined") return false;
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

function persist(v: boolean) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, v ? "1" : "0");
  } catch {
    /* ignore quota / privacy-mode errors */
  }
}

function createTerminalStore() {
  let open = $state(loadInitial());

  return {
    get open() {
      return open;
    },
    set open(v: boolean) {
      open = v;
      persist(v);
    },
    toggle() {
      open = !open;
      persist(open);
    },
  };
}

export const terminal = createTerminalStore();
