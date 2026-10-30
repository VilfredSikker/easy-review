// Per-viewer display preferences. They live in localStorage, beside the split
// and wrap choices, because they describe how this reader likes to look at a
// diff, not anything about the repo: config is global managed storage
// (ADR 0005) and is the wrong home for a presentation toggle.

export const HIDE_REVIEWED_KEY = "er.hideReviewed";
export const LINE_NUMBERS_KEY = "er.lineNumbers";
export const MARK_LINES_KEY = "er.markLines";

/** A stored flag, or `fallback` when it is unset or storage is unavailable. */
export function loadFlag(key: string, fallback: boolean): boolean {
  try {
    const v = globalThis.localStorage?.getItem(key);
    if (v === "1") return true;
    if (v === "0") return false;
  } catch {
    /* privacy mode or a blocked origin: use the default */
  }
  return fallback;
}

export function saveFlag(key: string, value: boolean): void {
  try {
    globalThis.localStorage?.setItem(key, value ? "1" : "0");
  } catch {
    /* quota or privacy mode: the toggle still applies for this session */
  }
}
