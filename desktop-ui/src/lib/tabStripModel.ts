import type { TabSummary } from "./types";
import type { ArenaRunSummary } from "./types/arena";

/** Trailing state mark on a tab. One at a time: a running review outranks a
 *  stale diff, which outranks unread changes, so the strip never stacks dots. */
export type TabMark = "running" | "stale" | "unseen" | null;

export interface TabRowModel {
  /** "#412" for PR tabs, else null. Rendered faint ahead of the title. */
  prefix: string | null;
  title: string;
  mark: TabMark;
}

export interface TabRowFacts {
  running: boolean;
  unseen: boolean;
}

/** The backend names a PR tab `repo#412` (TabState::tab_name); the prefix owns
 *  the number, so drop it from whichever end of the label carries it. */
function titleWithoutNumber(label: string, pr: number): string {
  const num = `#${pr}`;
  if (label.startsWith(`${num} `)) return label.slice(num.length + 1);
  if (label.endsWith(num) && label.length > num.length) return label.slice(0, -num.length);
  return label;
}

/** `summaries` holds only the active tab's branch; runs on other tabs reach
 *  the frontend solely through `background`. A run in both counts once. */
export function arenaRunsAcrossTabs(
  summaries: readonly ArenaRunSummary[],
  background: readonly ArenaRunSummary[],
): ArenaRunSummary[] {
  const seen = new Set(summaries.map((s) => s.id));
  return [...summaries, ...background.filter((b) => !seen.has(b.id))];
}

export function tabRowModel(tab: TabSummary, facts: TabRowFacts): TabRowModel {
  const pr = tab.pr_number;
  return {
    prefix: pr != null ? `#${pr}` : null,
    title: pr != null ? titleWithoutNumber(tab.label, pr) : tab.label,
    mark: markFor(tab, facts),
  };
}

function markFor(tab: TabSummary, facts: TabRowFacts): TabMark {
  if (facts.running) return "running";
  if (tab.stale) return "stale";
  if (facts.unseen && !tab.is_active) return "unseen";
  return null;
}
