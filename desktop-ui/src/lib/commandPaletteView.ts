// Presentation rules for the command palette that do not need the DOM: which
// glyph a row gets, how search results are grouped, which keys run an item,
// and the cross-repo PR list the PR menu offers.

import { baseName } from "./shortFileName";
import type { PrInfo, ProjectSnapshot } from "./types";

/** How long a run keeps its row flashed before the palette closes. */
export const RUN_FLASH_MS = 140;

export type PaletteGroup =
  | "Actions"
  | "Navigate"
  | "View & Layout"
  | "AI"
  | "AI · Advanced"
  | "PR"
  | "Files in this diff";

const GROUP_GLYPHS: Record<PaletteGroup, string> = {
  Actions: "⌘",
  Navigate: "→",
  "View & Layout": "▣",
  AI: "✦",
  "AI · Advanced": "✦",
  PR: "#",
  "Files in this diff": "·",
};

export function paletteGlyph(group: PaletteGroup): string {
  return GROUP_GLYPHS[group];
}

export function palettePlaceholder(menuLabel: string | null): string {
  return menuLabel ? `Search ${menuLabel}… or press a letter` : "Press a letter, or / to search";
}

/** A single character is the key that runs the row; chords only document a global shortcut. */
export function isSingleKey(kbd: string): boolean {
  return [...kbd].length === 1;
}

/**
 * Whether an unchorded key should run a row in view. `paletteQuickActionKey`
 * only knows letters; the menus also bind digits (1/2/3 pages) and
 * punctuation (`,` settings, `[` `]` panels), which would otherwise be
 * swallowed as stray typing.
 */
export function isRowKey(
  e: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean },
  searchFocused: boolean,
  bound: (key: string) => boolean,
): boolean {
  if (searchFocused || e.metaKey || e.ctrlKey || e.altKey) return false;
  if (e.key === "/" || e.key === " " || [...e.key].length !== 1) return false;
  return bound(e.key.toLowerCase());
}

export interface GroupedRow<T> {
  item: T;
  /** Set on the first row of each group. */
  header: string | null;
}

/**
 * Orders search hits so each menu's results sit together under one header,
 * menus in the order they first appear. The returned order is the one arrow
 * keys walk, so it must be the order the rows render in.
 */
export function groupByMenu<T extends { group: string }>(items: readonly T[]): GroupedRow<T>[] {
  const buckets = new Map<string, T[]>();
  for (const item of items) {
    const bucket = buckets.get(item.group);
    if (bucket) bucket.push(item);
    else buckets.set(item.group, [item]);
  }
  const rows: GroupedRow<T>[] = [];
  for (const [group, bucket] of buckets) {
    bucket.forEach((item, i) => rows.push({ item, header: i === 0 ? group : null }));
  }
  return rows;
}

/** Right-hand footer text naming what the palette acts on. */
export function paletteFooterContext(ctx: { repo: string; prNumber: number | null }): string {
  const parts = ["er"];
  if (ctx.repo) parts.push(ctx.prNumber != null ? `${ctx.repo} #${ctx.prNumber}` : ctx.repo);
  return parts.join(" · ");
}

export interface RecentPr {
  project: ProjectSnapshot;
  pr: PrInfo;
}

/**
 * Recently opened PRs across every project, newest first, without the one on
 * screen. Read from the snapshot's sidebar lists (ADR 0026): the palette never
 * asks GitHub itself.
 */
export function recentPrs(
  projects: readonly ProjectSnapshot[] | undefined,
  current: { projectId: string | null; prNumber: number | null },
  limit = 5,
): RecentPr[] {
  const seen = new Set<string>();
  const out: RecentPr[] = [];
  for (const project of projects ?? []) {
    for (const pr of project.recent_prs ?? []) {
      const key = `${project.id}#${pr.number}`;
      if (seen.has(key)) continue;
      seen.add(key);
      if (project.id === current.projectId && pr.number === current.prNumber) continue;
      out.push({ project, pr });
    }
  }
  out.sort((a, b) => b.pr.updated_at.localeCompare(a.pr.updated_at));
  return out.slice(0, limit);
}

/** Directory of a Files row, shown dimmed after its name. */
export function fileDir(path: string): string {
  const name = baseName(path);
  return path.length > name.length ? path.slice(0, path.length - name.length - 1) : "";
}
