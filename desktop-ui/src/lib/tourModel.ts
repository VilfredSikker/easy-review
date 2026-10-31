// Pure Tour geometry and lookups, kept out of FlatDiffView so the component
// only wires them to stores (ADR 0038 cohesion).

import { parseMarkdown, renderInline } from "./markdown";
import { filesByPillar } from "./pillarFiles";
import type { FileSnapshot, PillarSnapshot } from "./types";

/** What the render model needs to place one pillar-header row. */
export interface PillarHeaderSpec {
  pillarId: string;
  /** 1-based position in the tour. */
  index: number;
  total: number;
  title: string;
  narrativeMarkdown: string;
  /** The drawn file the header sits above; null puts it after every file. */
  anchorPath: string | null;
}

/**
 * One header per pillar, anchored on the first file that pillar owns. Ownership
 * follows `filesByPillar` (first claim wins), which is also how the Tour orders
 * files, so each header lands directly above its own files. A pillar with no
 * drawn file borrows the next pillar's anchor, keeping headers in tour order.
 */
export function pillarHeaderSpecs(
  pillars: readonly PillarSnapshot[],
  files: readonly FileSnapshot[],
): PillarHeaderSpec[] {
  if (pillars.length === 0) return [];
  const owned = new Map<string, string | null>();
  for (const g of filesByPillar(pillars, files)) {
    if (g.pillarId !== null) owned.set(g.pillarId, g.files[0]?.path ?? null);
  }
  const specs: PillarHeaderSpec[] = pillars.map((p, i) => ({
    pillarId: p.id,
    index: i + 1,
    total: pillars.length,
    title: p.title,
    narrativeMarkdown: p.descriptionMarkdown,
    anchorPath: owned.get(p.id) ?? null,
  }));
  let next: string | null = null;
  for (let i = specs.length - 1; i >= 0; i--) {
    if (specs[i].anchorPath === null) specs[i].anchorPath = next;
    else next = specs[i].anchorPath;
  }
  return specs;
}

/**
 * The pillar being read: the last header at or above `readingLinePx` (row
 * space). Above the first header the first pillar is active, so the rail never
 * shows nothing expanded.
 */
export function activePillarAt(
  pillarStartRow: ReadonlyMap<string, number>,
  cumulativeOffsets: readonly number[],
  readingLinePx: number,
): string | null {
  let best: string | null = null;
  let bestTop = -Infinity;
  let first: string | null = null;
  let firstTop = Infinity;
  for (const [id, row] of pillarStartRow) {
    const top = cumulativeOffsets[row] ?? 0;
    if (top < firstTop) {
      firstTop = top;
      first = id;
    }
    if (top <= readingLinePx && top > bestTop) {
      bestTop = top;
      best = id;
    }
  }
  return best ?? first;
}

/**
 * The row to scroll to when moving on to the file whose header is `fileRow`.
 * The first file of a pillar sits under that pillar's header, and landing on
 * the file would put the pillar's narrative above the viewport.
 */
export function fileEntryRow(rows: readonly { type: string }[], fileRow: number): number {
  let row = fileRow;
  while (row > 0 && rows[row - 1]?.type === "pillar-header") row--;
  return row;
}

/** Pillar ids in reading order. */
export function pillarOrder(pillarStartRow: ReadonlyMap<string, number>): string[] {
  return [...pillarStartRow.entries()].sort((a, b) => a[1] - b[1]).map(([id]) => id);
}

/** The pillar after `current`; stays on the last one, as the prototype does. */
export function nextPillarId(order: readonly string[], current: string | null): string | null {
  if (order.length === 0) return null;
  const i = current === null ? -1 : order.indexOf(current);
  return order[Math.min(order.length - 1, i + 1)];
}

/** A rail row: a pillar's file, or a related file nested under it. */
export interface RailFile {
  file: FileSnapshot;
  /** Related-file kind (test, style, story, snapshot); null for a primary. */
  relatedKind: string | null;
}

export interface RailPillar {
  id: string;
  title: string;
  summary: string;
  reviewed: number;
  total: number;
  files: RailFile[];
}

/**
 * The rail's pillars with their files in reading order. Counts come from the
 * snapshot, which owns them across views (ADR 0025); files a pillar shares
 * with an earlier one appear only under the first, matching the column.
 */
export function railPillars(
  pillars: readonly PillarSnapshot[],
  files: readonly FileSnapshot[],
): RailPillar[] {
  const owner = new Map<string, string>();
  for (const g of filesByPillar(pillars, files)) {
    if (g.pillarId === null) continue;
    for (const f of g.files) owner.set(f.path, g.pillarId);
  }
  const byPath = new Map(files.map((f) => [f.path, f]));
  return pillars.map((p) => {
    const rows: RailFile[] = [];
    const take = (path: string, relatedKind: string | null) => {
      const f = byPath.get(path);
      if (f && owner.get(path) === p.id && !rows.some((r) => r.file.path === path)) {
        rows.push({ file: f, relatedKind });
      }
    };
    for (const ref of p.files) {
      take(ref.path, null);
      for (const rel of ref.related ?? []) take(rel.path, rel.kind);
    }
    return {
      id: p.id,
      title: p.title,
      summary: p.summaryMarkdown?.trim() || p.descriptionMarkdown,
      reviewed: p.reviewedCount,
      total: p.totalCount,
      files: rows,
    };
  });
}

/** Two-digit progress count, as the rail prints it (`03/12`). */
export function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/**
 * The diff path a narrative reference names, or null. Accepts the full path,
 * a `./` or `/` prefix, a `:line` or `#L…` suffix, or a trailing path suffix
 * (a basename) when exactly one drawn file ends with it. Anything else stays
 * plain text: a reference must point into this diff, never outside it.
 */
export function resolveDiffPath(target: string, paths: readonly string[]): string | null {
  const t = target
    .trim()
    .replace(/^\.?\//, "")
    .replace(/(#L\d+(-L?\d+)?|:\d+(-\d+)?)$/, "");
  if (!t || /\s/.test(t) || /^[a-z]+:\/\//i.test(t)) return null;
  if (paths.includes(t)) return t;
  const hits = paths.filter((p) => p.endsWith(`/${t}`));
  return hits.length === 1 ? hits[0] : null;
}

const decodeEntities = (s: string) =>
  s.replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
const encodeAttr = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/**
 * Turns file references in already-rendered inline HTML into `.ref` links that
 * carry `data-goto`. Two forms count: a markdown link whose target is a diff
 * path (`[schema](db/schema.sql)`, left literal by the inline renderer, which
 * only links http URLs) and a code span naming one (`` `schema.sql` ``).
 */
export function linkFileRefs(html: string, paths: readonly string[]): string {
  // One alternation, so a code span inside a link label is consumed with the
  // link and never wrapped in a second anchor.
  return html.replace(
    /\[([^\]]+)\]\(([^)\s]+)\)|<code>([^<]+)<\/code>/g,
    (full, label: string | undefined, target: string | undefined, code: string | undefined) => {
      const ref = code ?? target ?? "";
      const path = resolveDiffPath(decodeEntities(ref), paths);
      if (path === null) return full;
      const text = code === undefined ? label : full;
      return `<a class="ref" href="#" data-goto="${encodeAttr(path)}">${text}</a>`;
    },
  );
}

/**
 * A pillar narrative as HTML with its file references linked. Every text run
 * goes through `renderInline`, which escapes it first, and code blocks are
 * escaped here; `linkFileRefs` adds anchors only, with escaped attributes.
 * Tables and heading levels collapse to paragraphs: a narrative is prose.
 */
export function narrativeHtml(markdown: string, paths: readonly string[]): string {
  const inline = (s: string) => linkFileRefs(renderInline(s), paths);
  const list = (tag: "ul" | "ol", items: string[]) =>
    `<${tag}>${items.map((it) => `<li>${inline(it)}</li>`).join("")}</${tag}>`;
  return parseMarkdown(markdown)
    .map((n) => {
      switch (n.t) {
        case "ul":
        case "ol":
          return list(n.t, n.items);
        case "code":
          return `<pre><code>${encodeAttr(n.v)}</code></pre>`;
        case "table":
          return `<p>${inline(n.header.join(" · "))}</p>`;
        default:
          return `<p>${inline(n.v)}</p>`;
      }
    })
    .join("");
}
