import type { MainViewMode } from "./stores/app.svelte";

/**
 * Where leaving the Inbox lands: the main view it was opened from, so Esc or a
 * second click on Inbox undoes the click that opened it. With nothing to go
 * back to it falls back to the review's first page.
 */
export function inboxReturnView(before: MainViewMode | null, overviewAvailable: boolean): MainViewMode {
  if (before && before !== "inbox") return before;
  return overviewAvailable ? "overview" : "diff";
}
