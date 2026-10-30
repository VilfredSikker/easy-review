import type { TabSummary } from "./types";

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

/** A PR tab's label can already lead with its number; the prefix owns it. */
function titleWithoutNumber(label: string, pr: number): string {
  const lead = `#${pr}`;
  return label.startsWith(`${lead} `) ? label.slice(lead.length + 1) : label;
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
