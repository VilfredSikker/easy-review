import { filesByPillar } from "./pillarFiles";
import type { FileSnapshot, PillarSnapshot } from "./types";

/** One file as a ruler sees it: a tick on the edge and a row in the popover. */
export interface RulerItem {
  path: string;
  current: boolean;
  reviewed: boolean;
  /** Comments, questions and findings on the file. */
  annotations: number;
  file: FileSnapshot;
}

export interface RulerGroup {
  /** Null for the Diff ruler, which is one flat list with no headings. */
  title: string | null;
  items: RulerItem[];
}

export type RulerFilter = (file: FileSnapshot) => boolean;

export function annotationTotal(f: FileSnapshot): number {
  return f.comment_count + f.question_count + f.finding_count;
}

function toItem(f: FileSnapshot, currentPath: string | null): RulerItem {
  return {
    path: f.path,
    current: f.path === currentPath,
    reviewed: f.reviewed,
    annotations: annotationTotal(f),
    file: f,
  };
}

/** The Diff ruler follows the file list the viewer sees, so it takes the same
 *  filter: a tick for a file the list hides would jump to nothing. */
export function fileMapGroups(
  files: readonly FileSnapshot[],
  currentPath: string | null,
  filter?: RulerFilter,
): RulerGroup[] {
  const kept = filter ? files.filter(filter) : files;
  return [{ title: null, items: kept.map((f) => toItem(f, currentPath)) }];
}

/** Tour pillars become numbered groups; files the tour left out trail as an
 *  unnumbered group so the ruler still covers the whole diff. */
export function tourGroups(
  pillars: readonly PillarSnapshot[],
  files: readonly FileSnapshot[],
  currentPath: string | null,
  filter?: RulerFilter,
): RulerGroup[] {
  const groups: RulerGroup[] = [];
  for (const g of filesByPillar(pillars, files)) {
    const kept = filter ? g.files.filter(filter) : g.files;
    if (kept.length === 0) continue;
    const idx = g.pillarId === null ? -1 : pillars.findIndex((p) => p.id === g.pillarId);
    groups.push({
      title: idx >= 0 ? `${idx + 1}. ${g.title}` : g.title,
      items: kept.map((f) => toItem(f, currentPath)),
    });
  }
  return groups;
}

export function rulerFileCount(groups: readonly RulerGroup[]): number {
  return groups.reduce((n, g) => n + g.items.length, 0);
}

/** Current beats annotated beats reviewed: where the viewer is matters most,
 *  and a reviewed file that still carries a comment is not finished business. */
export type TickState = "cur" | "has" | "rev" | "plain";

export function tickState(item: RulerItem): TickState {
  if (item.current) return "cur";
  if (item.annotations > 0) return "has";
  if (item.reviewed) return "rev";
  return "plain";
}

const TICK_HEIGHT = 2;
const GROUP_GAP = 6;
const MAX_TICK_GAP = 4;

/** Pitch between ticks, shrinking so a very large diff still fits the edge
 *  instead of running off screen; ticks only touch at 0, never overlap. */
export function tickGap(tickCount: number, groupCount: number, availablePx: number): number {
  if (tickCount <= 1 || availablePx <= 0) return MAX_TICK_GAP;
  const fixed = tickCount * TICK_HEIGHT + Math.max(0, groupCount - 1) * GROUP_GAP;
  const room = (availablePx - fixed) / (tickCount - 1);
  return Math.max(0, Math.min(MAX_TICK_GAP, Math.floor(room)));
}

/** Keeps a popover inside its container's vertical extent. */
export function clampPopoverTop(ideal: number, containerH: number, popH: number, margin = 4): number {
  const maxTop = Math.max(margin, containerH - popH - margin);
  return Math.max(margin, Math.min(ideal, maxTop));
}
