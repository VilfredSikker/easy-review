/**
 * Which Overview sections the reader folded. Remembered per section across
 * tabs and restarts, since what someone skips (bot links, agent logs) is a
 * habit rather than a property of one PR. Storage can be missing or throw;
 * the defaults still apply then.
 */

const KEY = "er.overview.sections";

function load(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    return parsed && typeof parsed === "object" ? (parsed as Record<string, boolean>) : {};
  } catch {
    return {};
  }
}

let state = $state<Record<string, boolean>>(load());

function set(key: string, open: boolean) {
  state = { ...state, [key]: open };
  try {
    localStorage.setItem(KEY, JSON.stringify(state));
  } catch {
    // Not remembered this time; the section still toggles.
  }
}

export const overviewCollapse = {
  isOpen(key: string, defaultOpen = true): boolean {
    return state[key] ?? defaultOpen;
  },
  toggle(key: string, defaultOpen = true) {
    set(key, !(state[key] ?? defaultOpen));
  },
  /** A link into a folded section unfolds it rather than scrolling to a stub. */
  open(key: string) {
    if (state[key] !== true) set(key, true);
  },
};
