<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app, type DiffViewMode } from "$lib/stores/app.svelte";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { diffScroll } from "$lib/stores/diffScroll.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { aiFindingFilter } from "$lib/stores/aiFindingFilter.svelte";
  import { aiReviewFilter } from "$lib/stores/aiReviewFilter.svelte";
  import DiffComposer from "./DiffComposer.svelte";
  import ComposerScrollBack from "./ComposerScrollBack.svelte";
  import FileHeaderRow from "./diff-rows/FileHeaderRow.svelte";
  import HunkHeaderRow from "./diff-rows/HunkHeaderRow.svelte";
  import UnifiedRow from "./diff-rows/UnifiedRow.svelte";
  import FoldLineRow from "./diff-rows/FoldLineRow.svelte";
  import SplitContentRow from "./diff-rows/SplitContentRow.svelte";
  import CompactedStubRow from "./diff-rows/CompactedStubRow.svelte";
  import LazyStubRow from "./diff-rows/LazyStubRow.svelte";
  import NoChangesRow from "./diff-rows/NoChangesRow.svelte";
  import ThreadRow from "./diff-rows/ThreadRow.svelte";
  import FindingRow from "./diff-rows/FindingRow.svelte";
  import StickyFileHeader from "./diff-rows/StickyFileHeader.svelte";
  import PillarRail from "./diff-rows/PillarRail.svelte";
  import ReferenceRuler from "./ReferenceRuler.svelte";
  import ReferenceUsagesPopover from "./ReferenceUsagesPopover.svelte";
  import DiffSearchBar from "./DiffSearchBar.svelte";
  import { refHighlight } from "$lib/stores/referenceHighlight.svelte";
  import {
    buildRulerMarks,
    collectMatches,
    usageContext,
    type MatchResult,
    type UsageContextLine,
    type UsageLine,
    type UsageSource,
  } from "$lib/referenceUsages";
  import {
    applyCollapsedFiles,
    computeUnifiedPairs,
    getCrossFileModel,
    type CrossFileModel,
    type CrossFileFlatRow,
    type PillarHeaderInfo,
  } from "$lib/diffRenderModel";
  import { diffFileCollapse } from "$lib/stores/diffFileCollapse.svelte";
  import { splitRows } from "$lib/splitRows";
  import {
    windowFromScrollVariable,
    rowIndexAtOffset,
    rowOffsetFromContentTopY,
    filePathAtContentTop,
    fileHeaderInView,
    stickyFileHeaderOverlayHidden,
    type EffectiveGeometry,
  } from "$lib/virtualWindow";
  import { buildAnnotationIndex } from "$lib/diffAnnotations";
  import { makeScrollThrottle } from "$lib/scrollThrottle";
  import { highlightCache, type HunkHighlight } from "$lib/highlightCache";
  import { fileNeedsSyntaxSpans, highlightFile } from "$lib/highlightFile";
  import {
    applyHunkSpansIfChanged,
    cacheWouldImproveFile,
    shouldSkipHighlightApply,
  } from "$lib/highlightPlan";
  import { warmHighlightWorker } from "$lib/highlightClient";
  import { syntaxThemeById } from "$lib/syntaxThemes";
  import { profileLog, profileLogRateLimited } from "$lib/profileLog";
  import { buildTree, flattenForNav } from "$lib/treeFromPaths";
  import type { AppSnapshot, FileSnapshot, LineSnapshot } from "$lib/types";

  /** Prevents highlight $effect from re-applying spans in a reactive loop. */
  const _spansAppliedKeys = new Set<string>();

  const COMPOSER_APPROX_HEIGHT_PX = 160;

  /** Empty scroll space below the last row so the last file's final lines clear
   *  the bottom chrome. Render-only — added to the .hscroll height, never to the
   *  geometry object (which drives the virtual window, ruler marks, and clamps). */
  const MIN_BOTTOM_PAD_PX = 320;

  const FIXED_HEIGHT_ROW_TYPES = new Set<CrossFileFlatRow["type"]>([
    "file-header",
    "hunk-header",
    "content-unified",
    "content-fold",
    "content-split",
    "compacted-stub",
    "lazy-stub",
    "no-changes",
  ]);

  function setsEqual(a: Set<string>, b: Set<string>): boolean {
    if (a.size !== b.size) return false;
    for (const x of a) if (!b.has(x)) return false;
    return true;
  }

  function evictSpanKeysForPath(filePath: string): void {
    const prefix = `${filePath}::`;
    for (const key of _spansAppliedKeys) {
      if (key.startsWith(prefix)) _spansAppliedKeys.delete(key);
    }
  }

  function syntaxHighlightingEnabled(): boolean {
    return true;
  }

  function unifiedLineSide(line: LineSnapshot): "old" | "new" {
    return line.kind === "del" ? "old" : "new";
  }

  interface Props {
    viewModeOverride?: DiffViewMode | null;
  }
  const { viewModeOverride = null }: Props = $props();

  const snapshot = $derived(app.snapshot);
  // Order files to match the tree (folders-first, alphabetical, single-child
  // folder chains collapsed). Reuses the memoized buildTree + flattenForNav
  // already used by keyboard nav (`j`/`k`), so tree and diff render in lockstep.
  const files = $derived.by<FileSnapshot[]>(() => {
    const all = snapshot?.files ?? [];
    if (all.length === 0) return [];
    const byPath = new Map(all.map((f) => [f.path, f]));
    // Guide mode: order files by the tour's pillar sequence so each pillar's
    // files are contiguous (its header is injected before the first one).
    if (snapshot?.mode === "tour" && snapshot.tour?.pillars?.length) {
      const out: FileSnapshot[] = [];
      const seen = new Set<string>();
      const pushPath = (path: string) => {
        const f = byPath.get(path);
        if (f && !seen.has(path)) {
          out.push(f);
          seen.add(path);
        }
      };
      for (const p of snapshot.tour.pillars) {
        for (const tf of p.files) {
          // Primary file, then its co-located related files directly after it.
          pushPath(tf.path);
          for (const r of tf.related ?? []) pushPath(r.path);
        }
      }
      for (const f of all) {
        if (!seen.has(f.path)) out.push(f);
      }
      return out;
    }
    const orderedPaths = flattenForNav(buildTree(all));
    const out: FileSnapshot[] = [];
    for (const p of orderedPaths) {
      const f = byPath.get(p);
      if (f) out.push(f);
    }
    return out;
  });

  const treeHidden = $derived(!snapshot?.panels.tree);
  const viewMode = $derived<DiffViewMode>(viewModeOverride ?? app.diffViewMode);
  const mode = $derived(snapshot?.mode ?? "branch");
  /** Guide/Diff toggle is offered once a tour exists for the current view
   *  context (PR vs local branch). */
  const tourAvailable = $derived(
    (snapshot?.features?.viewTour ?? true) && (snapshot?.tour?.available ?? false),
  );
  /** Whether the current view's guide is attached to the PR diff. Drives where
   *  the Diff toggle returns to so leaving the Guide doesn't switch diffs. */
  const tourIsPr = $derived(snapshot?.tour?.scope === "pr");
  /** False when new changes have landed since the guide was generated. */
  const tourFresh = $derived(snapshot?.tour?.fresh ?? true);
  /** PR number for returning to the PR diff from a PR-scoped guide. */
  const tourPrNumber = $derived(
    snapshot?.detected_pr_number ??
      snapshot?.github?.number ??
      snapshot?.pr?.number ??
      null,
  );
  function exitGuideToDiff() {
    if (mode !== "tour") return;
    if (tourIsPr) {
      void app.cmd("set_mode", { mode: "pr_diff", prNumber: tourPrNumber });
    } else {
      void app.cmd("set_mode", { mode: "branch" });
    }
  }

  let settingsOpen = $state(false);

  async function collapseAllDiffFiles() {
    diffFileCollapse.collapseAll(files.map((f) => f.path));
    await tick();
    if (!scrollEl) return;
    const maxTop = Math.max(0, effectiveGeometry.totalHeight - viewportHeightPx);
    if (scrollEl.scrollTop > maxTop) {
      scrollEl.scrollTop = maxTop;
    }
  }

  const snapshotKey = $derived(
    snapshot ? `${snapshot.active_tab}:${snapshot.mode}:${snapshot.base}:${snapshot.branch}` : mode,
  );

  const annotationIndex = $derived.by(() =>
    buildAnnotationIndex(
      snapshot?.ai ?? { threads: [], findings: [] },
      files,
      mode,
      app.commentVisibility,
      aiReviewFilter.filter,
      aiFindingFilter.severity,
    ),
  );
  const threadMap = $derived(annotationIndex.threadMap);

  // ── Cross-file model ───────────────────────────────────────────────────────
  const baseCrossFileModel = $derived(
    getCrossFileModel({
      files,
      viewMode,
      mode,
      annotationIndex,
      commentVisibility: app.commentVisibility,
      snapshotKey,
    }),
  );
  const crossFileModel = $derived.by(() => {
    diffFileCollapse.revision;
    return applyCollapsedFiles(baseCrossFileModel, diffFileCollapse.collapsed);
  });

  // Reset collapse state only when the diff context changes (tab/branch/mode).
  // clear() reads collapsed.size, so run it untracked — otherwise this effect
  // takes a dependency on `collapsed` and wipes every collapse the user makes.
  $effect(() => {
    snapshotKey;
    untrack(() => diffFileCollapse.clear());
  });

  // Clear the reference highlight when the diff context changes (tab/branch/PR/
  // mode). The store is a global singleton, so without this its query — and the
  // ruler marks derived from it — would persist into the next diff, e.g. stale
  // scroll-gutter marks carrying over when switching between PRs.
  $effect(() => {
    snapshotKey;
    untrack(() => refHighlight.clear());
  });

  // ── D10 measured-height overlay ───────────────────────────────────────────
  let overlayHeights = $state(new Map<string, number>());
  let overlaySerial = $state(0);

  // Guide mode: measured rendered height of each pillar's rail (keyed by pillar
  // id). Drives pillarPadByRowIdentity so a pillar's region is never shorter
  // than its rail (no overlap when files are short/collapsed).
  let railHeights = $state(new Map<string, number>());
  function setRailHeight(pillarId: string, px: number) {
    if ((railHeights.get(pillarId) ?? -1) === px) return;
    const next = new Map(railHeights);
    next.set(pillarId, px);
    railHeights = next;
  }
  /** Svelte action: report a node's rendered height (ResizeObserver). */
  function measureHeight(node: HTMLElement, onHeight: (px: number) => void) {
    let cb = onHeight;
    const ro = new ResizeObserver(() => cb(node.offsetHeight));
    ro.observe(node);
    cb(node.offsetHeight);
    return {
      update(next: (px: number) => void) { cb = next; },
      destroy() { ro.disconnect(); },
    };
  }

  // Evict stale overlay entries when the model changes to prevent unbounded growth.
  $effect(() => {
    const validIds = new Set(crossFileModel.rows.map((r) => r.identity));
    if (overlayHeights.size === 0) return;
    const next = new Map<string, number>();
    overlayHeights.forEach((v, k) => { if (validIds.has(k)) next.set(k, v); });
    if (next.size !== overlayHeights.size) {
      overlayHeights = next;
      overlaySerial++;
    }
  });

  function onHeightChange(identity: string, actualPx: number) {
    if (diffSel.dragging) return;
    const current = overlayHeights.get(identity);
    if (current === actualPx) return;
    const next = new Map(overlayHeights);
    next.set(identity, actualPx);
    overlayHeights = next;
    overlaySerial++;
  }

  // ── Effective geometry (model + overlay) ──────────────────────────────────
  // baseGeometry = model row heights + measured overlay heights (threads/findings),
  // WITHOUT the Guide pillar padding (so pillar-pad can be derived from it without
  // feeding back into itself).
  const baseGeometry = $derived.by<EffectiveGeometry>(() => {
    overlaySerial; // reactive on overlay changes
    const model = crossFileModel;
    if (model.rows.length === 0) {
      return { cumulativeOffsets: [0], totalHeight: 0, rowCount: 0 };
    }
    if (overlayHeights.size === 0) {
      return {
        cumulativeOffsets: model.cumulativeOffsets,
        totalHeight: model.totalHeight,
        rowCount: model.rows.length,
      };
    }
    const offsets = new Array<number>(model.rows.length + 1);
    offsets[0] = 0;
    for (let i = 0; i < model.rows.length; i++) {
      const h = overlayHeights.get(model.rows[i].identity) ?? model.rows[i].height;
      offsets[i + 1] = offsets[i] + h;
    }
    return {
      cumulativeOffsets: offsets,
      totalHeight: offsets[model.rows.length],
      rowCount: model.rows.length,
    };
  });

  // Guide mode: extra bottom padding for each pillar's LAST row so the pillar's
  // region is at least as tall as its (measured) rail — keyed by that row's
  // identity. Derived from baseGeometry (never effectiveGeometry) to avoid a
  // feedback loop. Empty outside tour mode.
  const pillarPadByRowIdentity = $derived.by<Map<string, number>>(() => {
    const pad = new Map<string, number>();
    if (!tourActive || railHeights.size === 0) return pad;
    const model = crossFileModel;
    const offsets = baseGeometry.cumulativeOffsets;
    // Pillar boundaries: first in-diff file's start row, in pillar order.
    const bounds: { pillarId: string; startRow: number }[] = [];
    const present = new Set(files.map((f) => f.path));
    for (const p of snapshot?.tour?.pillars ?? []) {
      const firstPath = p.files.map((tf) => tf.path).find((path) => present.has(path));
      if (!firstPath) continue;
      const startRow = model.fileStartRow.get(firstPath);
      if (startRow === undefined) continue;
      bounds.push({ pillarId: p.id, startRow });
    }
    bounds.sort((a, b) => a.startRow - b.startRow);
    for (let i = 0; i < bounds.length; i++) {
      const startRow = bounds[i].startRow;
      const endRow = i + 1 < bounds.length ? bounds[i + 1].startRow : model.rows.length; // exclusive
      if (endRow <= startRow) continue;
      const diffSpanPx = (offsets[endRow] ?? 0) - (offsets[startRow] ?? 0);
      const railPx = railHeights.get(bounds[i].pillarId) ?? 0;
      const extra = Math.max(0, railPx - diffSpanPx);
      if (extra > 0) {
        const lastRow = model.rows[endRow - 1];
        if (lastRow) pad.set(lastRow.identity, extra);
      }
    }
    return pad;
  });

  // effectiveGeometry = baseGeometry + Guide pillar padding. Everything
  // downstream (virtual window, pillarSpans, scroll mapping) uses this.
  const effectiveGeometry = $derived.by<EffectiveGeometry>(() => {
    const base = baseGeometry;
    const pad = pillarPadByRowIdentity;
    if (pad.size === 0) return base;
    const model = crossFileModel;
    const offsets = new Array<number>(model.rows.length + 1);
    offsets[0] = 0;
    for (let i = 0; i < model.rows.length; i++) {
      const baseH = base.cumulativeOffsets[i + 1] - base.cumulativeOffsets[i];
      offsets[i + 1] = offsets[i] + baseH + (pad.get(model.rows[i].identity) ?? 0);
    }
    return {
      cumulativeOffsets: offsets,
      totalHeight: offsets[model.rows.length],
      rowCount: model.rows.length,
    };
  });

  // ── Scroll + viewport ─────────────────────────────────────────────────────
  let scrollEl: HTMLDivElement | null = $state(null);
  let hscrollEl: HTMLDivElement | null = $state(null);
  let scrollTopPx = $state(0);
  /** Unthrottled scroll position for composer visibility / go-back pill. */
  let scrollTopLivePx = $state(0);
  let viewportHeightPx = $state(0);

  const _updateScrollTop = makeScrollThrottle((top) => { scrollTopPx = top; });

  let lastViewKey: string | null = null;
  let scrollSaveTimer: ReturnType<typeof setTimeout> | null = null;
  let treeFollowTimer: ReturnType<typeof setTimeout> | null = null;

  function onScroll() {
    if (!scrollEl) return;
    const top = scrollEl.scrollTop;
    scrollTopLivePx = top;
    _updateScrollTop(top);
    if (scrollSaveTimer) clearTimeout(scrollSaveTimer);
    const curKey = snapshotKey;
    scrollSaveTimer = setTimeout(() => {
      diffScroll.setScrollTop(curKey, top);
    }, 150);
    // Idle-debounce tree-follow: write currentFilePath only after 200ms scroll silence.
    if (treeFollowTimer) clearTimeout(treeFollowTimer);
    treeFollowTimer = setTimeout(() => {
      const idx = rowIndexAtOffset(effectiveGeometry, top);
      if (idx >= 0 && idx < crossFileModel.rows.length) {
        diffScroll.currentFilePath = crossFileModel.rows[idx].filePath ?? null;
      }
    }, 200);
  }

  $effect(() => {
    if (!scrollEl) return;
    if (lastViewKey === snapshotKey) return;
    // Consume pending scroll override (e.g. file-tree click that also triggers select_file).
    const pending = diffNav.pendingScrollPx;
    const top = pending !== null ? pending : diffScroll.getScrollTop(snapshotKey);
    diffNav.pendingScrollPx = null;
    // On any key change (tab/branch/mode), reset scroll to the stored position (0 for new keys)
    // and clear the applied-spans set so fresh hunks get highlighted rather than skipped.
    // Clear dead-stub memo too: source indices map to different files in the new context.
    _spansAppliedKeys.clear();
    _deadStubs.clear();
    lastViewKey = snapshotKey;
    tick().then(() => { if (scrollEl) scrollEl.scrollTop = top; });
  });

  // ── Virtual window ────────────────────────────────────────────────────────
  const OVERSCAN = 15;
  /**
   * Pixel band rendered beyond the viewport in each direction. Row-count overscan
   * alone can't keep up with a fast momentum flick (it jumps many rows per frame,
   * exposing the spacer as a black gap); ~1.5 viewport-heights of pre-rendered
   * pixels keeps the window ahead of the native scroll regardless of row heights.
   */
  const OVERSCAN_PX = $derived(Math.round(viewportHeightPx * 1.5));
  /** A screenful of empty scroll space appended below the last row (floored so
   *  it's never 0 before the viewport is measured). */
  const bottomPadPx = $derived(Math.max(MIN_BOTTOM_PAD_PX, viewportHeightPx));
  /** Sticky file-path bar in .vscroll (h-10) — row offsets live inside .hscroll below it. */
  const STICKY_HEADER_PX = 40;
  const rowScrollTopPx = $derived(Math.max(0, scrollTopPx - STICKY_HEADER_PX));
  const vw = $derived(
    windowFromScrollVariable(
      effectiveGeometry.cumulativeOffsets,
      effectiveGeometry.totalHeight,
      rowScrollTopPx,
      viewportHeightPx,
      OVERSCAN,
      OVERSCAN_PX,
    ),
  );
  const windowedRows = $derived(crossFileModel.rows.slice(vw.start, vw.end));

  // ── Visible file (for sticky header) ─────────────────────────────────────
  const visibleFilePath = $derived(
    filePathAtContentTop(effectiveGeometry, crossFileModel.rows, scrollTopLivePx),
  );

  const visibleFileHeaderRow = $derived.by((): Extract<CrossFileFlatRow, { type: "file-header" }> | null => {
    if (!visibleFilePath) return null;
    const startRow = crossFileModel.fileStartRow.get(visibleFilePath);
    if (startRow === undefined) return null;
    const row = crossFileModel.rows[startRow];
    return row?.type === "file-header" ? row : null;
  });

  // Hide sticky overlay when the real file-header row is in the top band of the row viewport.
  const stickyHeaderHidden = $derived.by(() => {
    if (!visibleFilePath) return false;
    const startRow = crossFileModel.fileStartRow.get(visibleFilePath);
    if (startRow === undefined) return false;
    const headerTop = effectiveGeometry.cumulativeOffsets[startRow] ?? 0;
    return stickyFileHeaderOverlayHidden(headerTop, scrollTopLivePx, STICKY_HEADER_PX);
  });

  /** Sticky overlay handles header clicks; in-flow headers must not sit underneath. */
  const stickyHeaderClicksOverlay = $derived(
    !stickyHeaderHidden && visibleFileHeaderRow !== null,
  );

  // ── Guide mode: Split View pillar lane ───────────────────────────────────
  const tourActive = $derived(snapshot?.mode === "tour");
  /** Width of the left pillar rail lane in Guide mode. */
  const RAIL_W = 320;

  /**
   * Vertical span of each pillar's diffs (column 2), so the left rail (column 1)
   * can position a sticky block aligned to it. Files are reordered contiguously
   * per pillar, so each span runs from its first file's start row to the next
   * pillar's. Uses post-collapse geometry so spans track collapsed heights.
   */
  const pillarSpans = $derived.by(
    (): { info: PillarHeaderInfo; topPx: number; heightPx: number }[] => {
      if (!tourActive || !snapshot?.tour?.pillars?.length) return [];
      const geom = effectiveGeometry;
      const model = crossFileModel;
      const present = new Set(files.map((f) => f.path));
      const entries: { info: PillarHeaderInfo; topPx: number }[] = [];
      for (const p of snapshot.tour.pillars) {
        const firstPath = p.files.map((tf) => tf.path).find((path) => present.has(path));
        if (!firstPath) continue;
        const startRow = model.fileStartRow.get(firstPath);
        if (startRow === undefined) continue;
        entries.push({
          info: {
            pillarId: p.id,
            title: p.title,
            descriptionMarkdown: p.descriptionMarkdown,
            reviewedCount: p.reviewedCount,
            totalCount: p.totalCount,
            foundation: p.foundation,
          },
          topPx: geom.cumulativeOffsets[startRow] ?? 0,
        });
      }
      entries.sort((a, b) => a.topPx - b.topPx);
      const total = geom.totalHeight;
      return entries.map((e, i) => ({
        info: e.info,
        topPx: e.topPx,
        heightPx: (i + 1 < entries.length ? entries[i + 1].topPx : total) - e.topPx,
      }));
    },
  );

  /** Per-pillar primary file rows for the rail (path + +/- + reviewed), in diff
   *  order. Co-located related files are excluded here and rendered nested via
   *  {@link relatedRows}. */
  const pillarFileRows = $derived.by((): Map<string, FileSnapshot[]> => {
    const m = new Map<string, FileSnapshot[]>();
    if (!tourActive || !snapshot?.tour?.pillars?.length) return m;
    const byPath = new Map(files.map((f) => [f.path, f]));
    for (const p of snapshot.tour.pillars) {
      const list: FileSnapshot[] = [];
      for (const tf of p.files) {
        const f = byPath.get(tf.path);
        if (f) list.push(f);
      }
      m.set(p.id, list);
    }
    return m;
  });

  /** Map of primary file path → its co-located related rows (test/style/…), for
   *  nested rendering in the pillar rail. */
  const relatedRows = $derived.by((): Map<string, { file: FileSnapshot; kind: string }[]> => {
    const m = new Map<string, { file: FileSnapshot; kind: string }[]>();
    if (!tourActive || !snapshot?.tour?.pillars?.length) return m;
    const byPath = new Map(files.map((f) => [f.path, f]));
    for (const p of snapshot.tour.pillars) {
      for (const tf of p.files) {
        const children: { file: FileSnapshot; kind: string }[] = [];
        for (const r of tf.related ?? []) {
          const f = byPath.get(r.path);
          if (f) children.push({ file: f, kind: r.kind });
        }
        if (children.length) m.set(tf.path, children);
      }
    }
    return m;
  });

  // Guide mode: auto-collapse a file to a compact row once it's reviewed. Acts
  // only on the false→true transition (and on already-reviewed files when Guide
  // is first entered), so a user who manually re-expands a reviewed file isn't
  // fought.
  let _prevReviewedTour = new Set<string>();
  $effect(() => {
    if (snapshot?.mode !== "tour") {
      _prevReviewedTour = new Set();
      return;
    }
    const cur = new Set<string>();
    for (const f of files) {
      if (!f.reviewed) continue;
      cur.add(f.path);
      if (!_prevReviewedTour.has(f.path)) diffFileCollapse.collapse(f.path);
    }
    _prevReviewedTour = cur;
  });

  // ── Selection validation ─────────────────────────────────────────────────
  let selectionContextKey: string | null = $state(null);

  $effect(() => {
    if (!diffSel.hasSelection || diffSel.file === null) {
      selectionContextKey = null;
      return;
    }

    const currentContextKey = snapshotKey;
    const fileStillRendered = crossFileModel.fileStartRow.has(diffSel.file);
    const contextChanged =
      selectionContextKey !== null && selectionContextKey !== currentContextKey;

    if (contextChanged || !fileStillRendered) {
      diffSel.clear();
      selectionContextKey = null;
      return;
    }

    selectionContextKey = currentContextKey;
  });

  // ── Lazy-load effect ──────────────────────────────────────────────────────
  const _requestingFiles = new Set<number>();
  /** Source indices that came back still-lazy after a parse attempt (no parseable
   *  hunks: binary, mode-only, rename-without-content, empty). Re-requesting can't
   *  produce hunks, so we never ask again — this avoids wasted IPC on every scroll
   *  re-run and closes a latent spin if a perpetual stub's cache_key ever churned.
   *  Cleared on context change (tab/branch/mode) alongside _spansAppliedKeys.
   *  Keyed by `sourceIndex:path` — indices can shift on a watch refresh within
   *  the same view, and a stale index-only entry would silently block another
   *  file's lazy load. */
  const _deadStubs = new Set<string>();
  /** Max distinct lazy files fetched per round-trip (bounds how long the backend
   *  holds the app mutex for one call). */
  const REQUEST_FILE_BATCH = 12;

  // `request_file_content` returns only the requested files' `FileSnapshot`s
  // (not the whole `AppSnapshot`), which we merge in place. This keeps the
  // viewport-driven lazy round-trip cheap on large diffs — a fast-scroll burst
  // that reveals several stubs is one call, not N full-snapshot serializations.
  async function requestLazyFiles(sourceIndices: number[]): Promise<void> {
    const fresh = sourceIndices.filter((i) => !_requestingFiles.has(i));
    if (fresh.length === 0) return;
    for (const i of fresh) _requestingFiles.add(i);
    const reqSnap = app.snapshot;
    const reqTab = reqSnap?.active_tab;
    const reqMode = reqSnap?.mode;
    const reqBase = reqSnap?.base;
    const reqBranch = reqSnap?.branch;
    try {
      const files = await invoke<FileSnapshot[]>("request_file_content", {
        sourceIndices: fresh,
      });
      if (!files || !app.snapshot) return;
      // Drop stale responses: the view changed while the round-trip was in flight.
      if (
        app.snapshot.active_tab !== reqTab ||
        app.snapshot.mode !== reqMode ||
        app.snapshot.base !== reqBase ||
        app.snapshot.branch !== reqBranch
      )
        return;
      for (const newFile of files) {
        const oldFile = app.snapshot.files.find((f) => f.source_index === newFile.source_index);
        if (!oldFile) continue;
        const prevCacheKey = oldFile.cache_key;
        oldFile.hunks = newFile.hunks;
        oldFile.is_lazy_stub = newFile.is_lazy_stub;
        oldFile.compacted = newFile.compacted;
        oldFile.additions = newFile.additions;
        oldFile.deletions = newFile.deletions;
        oldFile.cache_key = newFile.cache_key;
        // Keep delta_key in sync so later differential snapshots can omit
        // this file's hunks against the content we just received.
        oldFile.delta_key = newFile.delta_key;
        // Parsed but still a stub → no hunks will ever come from this file; don't
        // ask again until the context changes.
        if (newFile.is_lazy_stub) _deadStubs.add(`${newFile.source_index}:${newFile.path}`);
        // Only evict highlight spans when the hunks actually changed (cache_key
        // changed) — skipping eviction on unchanged hunks avoids a redundant flush.
        if (prevCacheKey !== newFile.cache_key) {
          evictSpanKeysForPath(oldFile.path);
        }
      }
      scheduleHighlightDrain();
    } finally {
      for (const i of fresh) _requestingFiles.delete(i);
    }
  }

  // Forward prefetch: also request lazy stubs *ahead* of the rendered window in
  // the scroll direction, so a fast flick lands on already-parsed content. The
  // look-ahead distance scales with per-frame scroll speed (the throttled
  // position delta between effect runs) and is clamped. This drives only
  // `request_file_content`, never `windowedRows` — so it fetches data ahead
  // without adding to per-frame render/mount cost.
  const PREFETCH_LOOKAHEAD_FACTOR = 4;
  const PREFETCH_MAX_VIEWPORTS = 3;
  let _prefetchLastTop = 0;

  function collectLazyStubsInRows(
    startRow: number,
    endRow: number,
    into: number[],
    seen: Set<number>,
  ): void {
    const rows = crossFileModel.rows;
    const hi = Math.min(rows.length, endRow);
    for (let i = Math.max(0, startRow); i < hi; i++) {
      if (into.length >= REQUEST_FILE_BATCH) return;
      const row = rows[i];
      if (row.type !== "lazy-stub") continue;
      if (
        _requestingFiles.has(row.sourceIndex) ||
        _deadStubs.has(`${row.sourceIndex}:${row.filePath ?? ""}`) ||
        seen.has(row.sourceIndex)
      )
        continue;
      seen.add(row.sourceIndex);
      into.push(row.sourceIndex);
    }
  }

  $effect(() => {
    const pending: number[] = [];
    const seen = new Set<number>();

    // 1) Stubs already inside the rendered window.
    collectLazyStubsInRows(vw.start, vw.end, pending, seen);

    // 2) Forward look-ahead band, sized by recent scroll speed and direction.
    const delta = rowScrollTopPx - _prefetchLastTop;
    _prefetchLastTop = rowScrollTopPx;
    if (pending.length < REQUEST_FILE_BATCH && delta !== 0) {
      const lookAheadPx = Math.min(
        Math.abs(delta) * PREFETCH_LOOKAHEAD_FACTOR,
        viewportHeightPx * PREFETCH_MAX_VIEWPORTS,
      );
      if (delta > 0) {
        const bandTop = rowScrollTopPx + viewportHeightPx + OVERSCAN_PX;
        collectLazyStubsInRows(
          rowIndexAtOffset(effectiveGeometry, bandTop),
          rowIndexAtOffset(effectiveGeometry, bandTop + lookAheadPx) + 1,
          pending,
          seen,
        );
      } else {
        const bandBottom = Math.max(0, rowScrollTopPx - OVERSCAN_PX);
        collectLazyStubsInRows(
          rowIndexAtOffset(effectiveGeometry, Math.max(0, bandBottom - lookAheadPx)),
          rowIndexAtOffset(effectiveGeometry, bandBottom) + 1,
          pending,
          seen,
        );
      }
    }

    if (pending.length > 0) void requestLazyFiles(pending);
  });

  // ── Highlight effect (Shiki web worker) ───────────────────────────────────
  const syntaxTheme = $derived(syntaxThemeById(app.currentSyntaxTheme));
  // Reactive pulse used only to drain the bounded highlight queue after worker
  // completions or lazy-file loads. Normal viewport/file changes remain direct deps.
  let _highlightQueuePulse = $state(0);
  let _highlightDrainScheduled = false;
  const _highlightQueued = new Set<string>();
  const _highlightInFlight = new Set<string>();
  const _highlightCompletedKeys = new Set<string>();
  const _highlightFailedKeys = new Set<string>();
  let _lastSnapshotRef: AppSnapshot | null = null;
  let _visibleFilePaths = $state(new Set<string>());
  /** Bumped when client-side syntax spans land on live hunks (invalidates virtual row lookups). */
  let _syntaxRevision = $state(0);

  onMount(() => {
    if (syntaxHighlightingEnabled()) {
      warmHighlightWorker(syntaxThemeById(app.currentSyntaxTheme));
    }
  });

  function isFileInViewport(filePath: string): boolean {
    return _visibleFilePaths.has(filePath);
  }

  const snapFileNeedsSpans = fileNeedsSyntaxSpans;

  function scheduleHighlightDrain(): void {
    if (_highlightDrainScheduled) return;
    _highlightDrainScheduled = true;
    const run = () => {
      _highlightDrainScheduled = false;
      _highlightQueuePulse++;
    };
    const requestIdle = globalThis.requestIdleCallback;
    if (typeof requestIdle === "function") {
      requestIdle(run, { timeout: 50 });
    } else {
      setTimeout(run, 0);
    }
  }

  function hunksHaveColor(hunks: HunkHighlight[]): boolean {
    for (const h of hunks) {
      for (const lineSpans of h.lines) {
        if (lineSpans.some((s) => s.color)) return true;
      }
    }
    return false;
  }

  /** Apply spans when they change live lines; returns true when hunks were updated. */
  function tryApplyHighlightSpans(
    snapFile: FileSnapshot,
    hunks: HunkHighlight[],
    spanKey: string,
  ): boolean {
    if (shouldSkipHighlightApply(snapFile, _spansAppliedKeys.has(spanKey))) return false;
    if (!hunksHaveColor(hunks)) {
      _spansAppliedKeys.add(spanKey);
      return false;
    }
    if (!snapFileNeedsSpans(snapFile)) {
      _spansAppliedKeys.add(spanKey);
      return false;
    }

    const changed = applyHunkSpansIfChanged(snapFile, hunks);
    if (changed) {
      _spansAppliedKeys.add(spanKey);
      _syntaxRevision += 1;
      return true;
    }

    // Cache apply made no progress — stop cache re-apply; fall through to worker if still needed.
    if (!cacheWouldImproveFile(snapFile, hunks)) {
      _spansAppliedKeys.add(spanKey);
    } else {
      _spansAppliedKeys.delete(spanKey);
    }
    return false;
  }

  function setVisibleFilePaths(next: Set<string>): void {
    if (setsEqual(_visibleFilePaths, next)) return;
    _visibleFilePaths = next;
  }

  // Evict highlight keys only when a file's cache_key changes (not on every poll chrome merge).
  let _lastFileCacheKeys = new Map<string, string>();
  let _lastSyntaxThemeId = "";
  $effect(() => {
    const themeId = syntaxTheme.id;
    if (snapshot === _lastSnapshotRef && themeId === _lastSyntaxThemeId) return;
    _lastSnapshotRef = snapshot;
    const list = snapshot?.files ?? [];
    let evicted = 0;
    const previousThemeId = _lastSyntaxThemeId;
    _lastSyntaxThemeId = themeId;
    const nextKeys = new Map<string, string>();
    for (const f of list) {
      nextKeys.set(f.path, f.cache_key);
      const prevKey = _lastFileCacheKeys.get(f.path);
      if (prevKey !== undefined && prevKey !== f.cache_key && fileNeedsSyntaxSpans(f)) {
        const key = highlightCache.key(f.path, prevKey, previousThemeId);
        highlightCache.delete(key);
        if (_spansAppliedKeys.delete(key)) evicted += 1;
      }
      if (previousThemeId !== themeId) {
        const key = highlightCache.key(f.path, f.cache_key, previousThemeId);
        highlightCache.delete(key);
        if (_spansAppliedKeys.delete(key)) evicted += 1;
      }
    }
    for (const path of _lastFileCacheKeys.keys()) {
      if (!nextKeys.has(path)) {
        const prevKey = _lastFileCacheKeys.get(path)!;
        const key = highlightCache.key(path, prevKey, previousThemeId);
        highlightCache.delete(key);
        if (_spansAppliedKeys.delete(key)) evicted += 1;
      }
    }
    _lastFileCacheKeys = nextKeys;
    if (evicted > 0) {
      profileLog("span_keys_evicted", { evicted_count: evicted });
    }
  });

  // Drop per-key highlight state when files leave the snapshot, cache_key changes, or theme changes.
  $effect(() => {
    const list = snapshot?.files ?? [];
    const valid = new Set(
      list.map((f) => highlightCache.key(f.path, f.cache_key, syntaxTheme.id)),
    );
    for (const key of _spansAppliedKeys) {
      if (!valid.has(key)) _spansAppliedKeys.delete(key);
    }
    for (const key of _highlightFailedKeys) {
      if (!valid.has(key)) _highlightFailedKeys.delete(key);
    }
    for (const key of _highlightCompletedKeys) {
      if (!valid.has(key)) _highlightCompletedKeys.delete(key);
    }
    for (const key of _highlightQueued) {
      if (!valid.has(key)) _highlightQueued.delete(key);
    }
  });

  $effect(() => {
    _highlightQueuePulse; // reactive dep: scheduled queue drain
    const rows = windowedRows;
    const visiblePaths = new Set(rows.map((r) => r.filePath));
    setVisibleFilePaths(visiblePaths);
    if (!syntaxHighlightingEnabled()) return;
    let queued = 0;
    let skippedApply = 0;
    let cacheApplied = 0;
    let cacheApplySkipped = 0;
    let failedSkipped = 0;
    let dedupeSkipped = 0;
    let concurrencySkipped = 0;
    for (const filePath of visiblePaths) {
      const file = files.find((f) => f.path === filePath);
      if (!file || file.is_lazy_stub || file.hunks.length === 0) continue;
      const spanKey = highlightCache.key(file.path, file.cache_key, syntaxTheme.id);
      const snapFile = app.snapshot?.files?.find((f) => f.path === file.path);
      if (!snapFile) continue;
      if (shouldSkipHighlightApply(snapFile, _spansAppliedKeys.has(spanKey))) {
        skippedApply += 1;
        continue;
      }

      const cachedHunks = highlightCache.get(spanKey);
      if (cachedHunks) {
        const cacheExhausted =
          _spansAppliedKeys.has(spanKey) && !cacheWouldImproveFile(snapFile, cachedHunks);
        if (cacheExhausted) {
          // Cached spans are applied and can't be improved — this spanKey is fully
          // resolved. Mark it terminal so the drain pulse can't re-queue the worker
          // forever for files with an uncolorable line (blank/whitespace/unknown
          // lang), whose fileNeedsSyntaxSpans never flips false. spanKey encodes
          // path+cache_key+theme, so a real diff/theme change yields a new key (and
          // the eviction effects drop the stale one) — legit re-highlight still runs.
          _highlightCompletedKeys.add(spanKey);
          continue;
        }
        const changed = untrack(() => tryApplyHighlightSpans(snapFile, cachedHunks, spanKey));
        if (changed) cacheApplied += 1;
        else cacheApplySkipped += 1;
        continue;
      }

      if (_highlightCompletedKeys.has(spanKey)) continue;
      if (_highlightFailedKeys.has(spanKey)) {
        failedSkipped += 1;
        continue;
      }
      if (_highlightQueued.has(spanKey) || _highlightInFlight.has(spanKey)) {
        dedupeSkipped += 1;
        continue;
      }
      if (_highlightInFlight.size >= 4) {
        concurrencySkipped += 1;
        continue;
      }
      _highlightQueued.add(spanKey);
      _highlightInFlight.add(spanKey);
      queued += 1;
      const requestCacheKey = file.cache_key;
      const tHighlight = performance.now();
      profileLog("highlight_start", {
        file: file.path,
        key: spanKey,
        visible_files: visiblePaths.size,
        in_flight: _highlightInFlight.size,
      });
      highlightFile(file, syntaxTheme)
        .then((hunks) => {
          _highlightQueued.delete(spanKey);
          _highlightInFlight.delete(spanKey);
          scheduleHighlightDrain();
          profileLog("highlight_done", {
            file: file.path,
            highlight_ms: Math.round(performance.now() - tHighlight),
            lines: hunks.reduce((n, h) => n + h.lines.length, 0),
          });
          if (!app.snapshot) return;
          const live = app.snapshot.files?.find((f) => f.path === file.path);
          if (!live || requestCacheKey !== live.cache_key) return;
          if (!hunksHaveColor(hunks)) {
            _highlightFailedKeys.add(spanKey);
            profileLog("highlight_failed_key", {
              file: file.path,
              key: spanKey,
              reason: "no_color",
            });
            return;
          }
          highlightCache.set(spanKey, hunks);
          if (!isFileInViewport(file.path)) return;
          const changed = untrack(() => tryApplyHighlightSpans(live, hunks, spanKey));
          if (changed || !snapFileNeedsSpans(live)) {
            _highlightCompletedKeys.add(spanKey);
          }
          profileLog("highlight_apply", {
            file: file.path,
            key: spanKey,
            changed: changed ? 1 : 0,
          });
        })
        .catch(() => {
          _highlightQueued.delete(spanKey);
          _highlightInFlight.delete(spanKey);
          _highlightFailedKeys.add(spanKey);
          profileLog("highlight_failed_key", {
            file: file.path,
            key: spanKey,
            reason: "worker_error",
          });
          scheduleHighlightDrain();
        });
    }
    profileLogRateLimited("highlight_effect", {
      visible_files: visiblePaths.size,
      queued,
      in_flight: _highlightInFlight.size,
      queued_keys: _highlightQueued.size,
      failed_keys: _highlightFailedKeys.size,
      completed_keys: _highlightCompletedKeys.size,
      applied_keys: _spansAppliedKeys.size,
      skipped_apply: skippedApply,
      cache_applied: cacheApplied,
      cache_apply_skipped: cacheApplySkipped,
      failed_skipped: failedSkipped,
      dedupe_skipped: dedupeSkipped,
      concurrency_skipped: concurrencySkipped,
    }, 5);
    if (queued > 0 || skippedApply > 0) {
      profileLog("highlight_queue", {
        visible_files: visiblePaths.size,
        queued,
        in_flight: _highlightInFlight.size,
        skipped_apply: skippedApply,
      });
    }
  });

  // ── Composer position ─────────────────────────────────────────────────────
  const composerTopPx = $derived.by(() => {
    if (!diffSel.hasSelection || diffSel.file === null || diffSel.end === null) return undefined;
    const fileStartRow = crossFileModel.fileStartRow.get(diffSel.file);
    if (fileStartRow === undefined) return undefined;
    // Place composer below the last selected line row
    const lastLn = diffSel.last();
    const fileRows = crossFileModel.rows;
    for (let i = fileStartRow; i < fileRows.length; i++) {
      const row = fileRows[i];
      if (row.filePath !== diffSel.file) break;
      if (
        (row.type === "content-unified" || row.type === "content-split") &&
        i < effectiveGeometry.cumulativeOffsets.length - 1
      ) {
        const file = files.find((f) => f.path === diffSel.file);
        if (!file) continue;
        let lineNum: number | null = null;
        if (row.type === "content-unified") {
          const ln = file.hunks[row.hunkIdx]?.lines[row.lineIdx];
          if (ln && unifiedLineSide(ln) === diffSel.side) {
            lineNum = ln.new_num ?? ln.old_num ?? null;
          }
        } else {
          const splitRowsByHunk = crossFileModel.splitRowsByFile.get(diffSel.file);
          const sr = splitRowsByHunk?.[row.hunkIdx]?.[row.splitRowIdx];
          const activeSide = diffSel.side === "old" ? sr?.left : sr?.right;
          lineNum = activeSide ? (activeSide.new_num ?? activeSide.old_num ?? null) : null;
        }
        if (lineNum === lastLn) {
          // +40: StickyFileHeader is always h-10 in layout (visibility:hidden, not display:none)
          // +8: breathing room so composer doesn't butt against the clicked line
          return effectiveGeometry.cumulativeOffsets[i + 1] + 40 + 8;
        }
      }
    }
    return undefined;
  });

  // ── Composer scroll: one-shot into view on open; free scroll afterward ───
  let composerAutoScrolledKey = $state<string | null>(null);

  const composerVisible = $derived.by(() => {
    if (!diffSel.composerOpen || composerTopPx === undefined) return true;
    const viewTop = scrollTopLivePx + STICKY_HEADER_PX;
    const viewBottom = scrollTopLivePx + viewportHeightPx;
    return viewTop <= composerTopPx + COMPOSER_APPROX_HEIGHT_PX && viewBottom >= composerTopPx;
  });

  const showGoBackToComment = $derived(
    diffSel.composerOpen && composerTopPx !== undefined && !composerVisible,
  );

  function scrollComposerIntoView() {
    const top = composerTopPx;
    if (top === undefined || !scrollEl) return;
    const LINE_H = 20;
    const selectedLineTop = top - LINE_H;
    scrollEl.scrollTop = Math.max(0, selectedLineTop - Math.floor(viewportHeightPx * 0.25));
  }

  $effect(() => {
    const top = composerTopPx;
    const key = diffSel.selectionKey();
    if (top === undefined || !scrollEl || !key || !diffSel.composerOpen) return;
    if (composerAutoScrolledKey === key) return;
    composerAutoScrolledKey = key;
    const st = scrollEl.scrollTop;
    const viewBottom = st + viewportHeightPx;
    const LINE_H = 20;
    const selectedLineTop = top - LINE_H;
    const wouldScroll = selectedLineTop > st + viewportHeightPx * 0.5 || top > viewBottom;
    if (wouldScroll) scrollComposerIntoView();
  });

  $effect(() => {
    if (!diffSel.hasSelection) composerAutoScrolledKey = null;
  });

  function applyScrollTop(top: number) {
    if (!scrollEl) return;
    const nextTop = Math.max(0, top);
    scrollEl.scrollTop = nextTop;
    scrollTopLivePx = nextTop;
    scrollTopPx = nextTop;
    diffScroll.setScrollTop(snapshotKey, nextTop);
    diffNav.pendingScrollPx = nextTop;
  }

  // ── Reference-highlight usages: overview ruler + Cmd+click popover ───────
  // (issue #69). Collected over ALL model rows (not just the viewport): the
  // ruler shows where every match lives in the scrollable content, and the
  // popover lists them. Only computed while a highlight is active.
  const RULER_MARK_HEIGHT_PX = 3;

  // Collect usage sources from the FULL diff — every file, including ones the
  // user has collapsed — so the references popover reflects the whole diff, not
  // just what is currently rendered. Each line's `rowIdx` is its position in
  // the live (post-collapse) render model, or -1 when its file is collapsed
  // (and thus has no rendered row); the (filePath, hunkIdx, lineIdx) anchor
  // lets `jumpToUsageLine` expand the file and re-resolve the row on click.
  function collectUsageSources(): UsageSource[] {
    // Map each rendered line's LineSnapshot to its current row index. Using
    // object identity covers both unified rows and split rows (whose left/right
    // sides reuse the same LineSnapshot objects from the hunk).
    const lineToRow = new Map<LineSnapshot, number>();
    const byPath = new Map(files.map((f) => [f.path, f]));
    const rows = crossFileModel.rows;
    for (let i = 0; i < rows.length; i++) {
      const row = rows[i];
      if (row.type === "content-unified") {
        const line = byPath.get(row.filePath)?.hunks[row.hunkIdx]?.lines[row.lineIdx];
        if (line) lineToRow.set(line, i);
      } else if (row.type === "content-split") {
        const sr =
          crossFileModel.splitRowsByFile.get(row.filePath)?.[row.hunkIdx]?.[row.splitRowIdx];
        if (sr?.left) lineToRow.set(sr.left, i);
        if (sr?.right) lineToRow.set(sr.right, i);
      }
    }

    const out: UsageSource[] = [];
    for (const file of files) {
      for (let h = 0; h < file.hunks.length; h++) {
        const lines = file.hunks[h].lines;
        for (let l = 0; l < lines.length; l++) {
          const line = lines[l];
          if (line.kind === "fold") continue;
          out.push({
            rowIdx: lineToRow.get(line) ?? -1,
            filePath: file.path,
            lineNum: line.new_num ?? line.old_num,
            text: line.text,
            hunkIdx: h,
            lineIdx: l,
          });
        }
      }
    }
    return out;
  }

  /** Current render-model row for a usage, re-resolved from its stable anchor
   *  (filePath, hunkIdx, lineIdx). -1 when the line still has no rendered row
   *  (file collapsed or not yet loaded). */
  function resolveUsageRow(u: Pick<UsageSource, "filePath" | "hunkIdx" | "lineIdx">): number {
    const match = usageSourcesAll.find(
      (s) => s.filePath === u.filePath && s.hunkIdx === u.hunkIdx && s.lineIdx === u.lineIdx,
    );
    return match?.rowIdx ?? -1;
  }

  /** Upper bound on collected matches — a one-letter Cmd+F query over a huge
   *  diff must not build an unbounded match list. */
  const SEARCH_MATCH_CAP = 5000;

  // Flat line list, only materialized while a highlight is active.
  // Full diff (all files, including collapsed) — drives the references popover
  // and the context previews (popover row expansion, ruler hover).
  const usageSourcesAll = $derived.by((): UsageSource[] =>
    refHighlight.identifier ? collectUsageSources() : [],
  );
  // Rendered subset — drives the overview ruler and Cmd+F search, both of which
  // can only scroll to a row that exists in the live render model. Sorted by
  // rowIdx because both consumers require ascending render order: the ruler's
  // buildRulerMarks merges by increasing offset, and Cmd+F steps linearly. The
  // full-diff order is file→hunk→line, which is non-monotonic in split view
  // (a modify block's del and add lines share a row but are visited apart);
  // the stable sort restores render order while keeping del before add.
  const usageSources = $derived(
    usageSourcesAll.filter((s) => s.rowIdx >= 0).sort((a, b) => a.rowIdx - b.rowIdx),
  );

  // Identifier highlights and Cmd+F queries share this pipeline; the store's
  // matchOptions switch between whole-word and substring/smart-case matching.
  const usageResult = $derived.by((): MatchResult => {
    const ident = refHighlight.identifier;
    if (!ident) return { lines: [], total: 0, capped: false };
    return collectMatches(usageSources, ident, refHighlight.matchOptions, SEARCH_MATCH_CAP);
  });
  const usageLines = $derived(usageResult.lines);

  // Popover usages span the whole diff (collapsed files included).
  const usageLinesAll = $derived.by((): UsageLine[] => {
    const ident = refHighlight.identifier;
    if (!ident) return [];
    return collectMatches(usageSourcesAll, ident, refHighlight.matchOptions, SEARCH_MATCH_CAP)
      .lines;
  });

  /** Context window the Cmd+click popover reveals when a usage row is
   *  expanded. Wider than the ruler-hover tooltip's compact peek so a
   *  multi-line construct (a function definition, a block) shows its body
   *  inline instead of just the signature's immediate neighbors. */
  const POPOVER_CONTEXT_LINES = 8;

  /** Context lines around a usage (ruler hover popover, popover expansion).
   *  Resolved over the full-diff sources so context works for usages in
   *  collapsed files too. */
  function contextForUsage(u: UsageLine, contextLines?: number): UsageContextLine[] {
    return usageContext(usageSourcesAll, u, contextLines);
  }

  /** First matched line at a ruler mark's row, with its surrounding context. */
  function previewForMarkRow(
    rowIdx: number,
  ): { usage: UsageLine; lines: UsageContextLine[] } | null {
    const usage = usageLines.find((u) => u.rowIdx === rowIdx);
    if (!usage) return null;
    return { usage, lines: contextForUsage(usage) };
  }

  const usageMarks = $derived.by(() => {
    if (usageLines.length === 0 || viewportHeightPx <= 0) return [];
    const offsets = effectiveGeometry.cumulativeOffsets;
    // Row offsets live below the in-flow sticky header band; the scrollable
    // content height is that band plus all rows.
    return buildRulerMarks(
      usageLines.map((u) => ({
        rowIdx: u.rowIdx,
        offsetPx: (offsets[u.rowIdx] ?? 0) + STICKY_HEADER_PX,
      })),
      effectiveGeometry.totalHeight + STICKY_HEADER_PX,
      viewportHeightPx,
      RULER_MARK_HEIGHT_PX,
    );
  });

  /**
   * Pulse a rendered row with the existing jump-to flash ring. Retries across
   * a few frames when the row element is not in the DOM yet: a far jump lands
   * in freshly-virtualized rows, and entry points that flush extra DOM work in
   * the same pass (the usages popover unmounting itself on click) could miss
   * the single-rAF window and silently skip the ring. The retry makes the ring
   * a guarantee of the jump, not a race — identical from every entry point.
   */
  function flashRowEl(rowIdx: number, attemptsLeft = 8): void {
    const rowEl = scrollEl?.querySelector<HTMLElement>(`[data-row-idx="${rowIdx}"]`);
    if (!rowEl) {
      if (attemptsLeft > 0) {
        requestAnimationFrame(() => flashRowEl(rowIdx, attemptsLeft - 1));
      }
      return;
    }
    rowEl.classList.remove("flash");
    // Force a reflow so the animation restarts when jumping to the same row twice.
    void rowEl.offsetWidth;
    rowEl.classList.add("flash");
    setTimeout(() => rowEl.classList.remove("flash"), 1300);
  }

  /**
   * THE shared reference jump: center the match row in the viewport and pulse
   * it with the `.flash` ring. Every reference entry point routes through this
   * one call — ruler-mark clicks, usages-popover rows (click and Enter), and
   * Cmd+F match navigation — so the scroll + ring treatment is identical
   * everywhere. Do not add a second jump/flash variant.
   */
  async function jumpToUsage(rowIdx: number): Promise<void> {
    const top = effectiveGeometry.cumulativeOffsets[rowIdx] ?? 0;
    applyScrollTop(Math.max(0, top - viewportHeightPx / 2));
    await tick();
    requestAnimationFrame(() => flashRowEl(rowIdx));
  }

  /**
   * Jump from a usage row in the references popover. A usage may live in a
   * collapsed (or not-yet-loaded) file, which has no rendered row — expand and
   * load it first, then re-resolve the row from the usage's stable anchor and
   * route through the shared `jumpToUsage` so the scroll + flash are identical.
   */
  async function jumpToUsageLine(u: UsageLine): Promise<void> {
    let rebuilt = false;
    if (diffFileCollapse.isCollapsed(u.filePath)) {
      diffFileCollapse.expand(u.filePath);
      rebuilt = true;
    }
    const file = files.find((f) => f.path === u.filePath);
    if (file?.is_lazy_stub) {
      await requestLazyFiles([file.source_index]);
      rebuilt = true;
    }
    if (rebuilt) {
      // Let the model re-derive and the new rows lay out before reading offsets.
      await tick();
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      );
    }
    const rowIdx = rebuilt ? resolveUsageRow(u) : u.rowIdx;
    if (rowIdx < 0) return;
    await jumpToUsage(rowIdx);
  }

  // ── Cmd+F search navigation (PR #73) ──────────────────────────────────────
  // Flat list of match row indices, one entry per range (a line with three
  // matches contributes three stops). Only materialized while the bar is open.
  const searchMatches = $derived.by((): number[] => {
    if (!refHighlight.searchOpen) return [];
    const out: number[] = [];
    for (const u of usageLines) {
      for (let i = 0; i < u.ranges.length; i++) out.push(u.rowIdx);
    }
    return out;
  });

  /** Enter/arrows: step through matches with wrap-around and flash the row. */
  function navigateSearch(dir: 1 | -1): void {
    const matches = searchMatches;
    if (matches.length === 0) return;
    const cur = refHighlight.searchActiveIdx;
    let next: number;
    if (cur < 0 || cur >= matches.length) {
      next = dir === 1 ? 0 : matches.length - 1;
    } else {
      next = (cur + dir + matches.length) % matches.length;
    }
    refHighlight.searchActiveIdx = next;
    void jumpToUsage(matches[next]);
  }

  // Clamp the active index when the match list shrinks (query edits, diff refresh).
  $effect(() => {
    const len = searchMatches.length;
    if (refHighlight.searchActiveIdx >= len) {
      refHighlight.searchActiveIdx = len === 0 ? -1 : len - 1;
    }
  });

  function scrollToFileHeader(path: string): boolean {
    const rowIdx = crossFileModel.fileStartRow.get(path);
    if (rowIdx === undefined) return false;
    const top = effectiveGeometry.cumulativeOffsets[rowIdx] ?? 0;
    applyScrollTop(top);
    return true;
  }

  async function scrollAfterCollapse(collapsedPath: string): Promise<void> {
    if (!scrollEl) return;
    await tick();
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
    const i = files.findIndex((f) => f.path === collapsedPath);
    if (i === -1) return;
    const target = files[i + 1] ?? files[i];
    const rowIdx = crossFileModel.fileStartRow.get(target.path);
    if (rowIdx === undefined) return;
    const top = effectiveGeometry.cumulativeOffsets[rowIdx] ?? 0;
    // Skip the scroll (and the diff-viewport repaint it triggers) when the next
    // header is already on screen below the sticky overlay.
    const inView = fileHeaderInView(top, scrollEl.scrollTop, scrollEl.clientHeight, STICKY_HEADER_PX);
    if (!inView) applyScrollTop(top);
    const file = files.find((f) => f.path === target.path);
    if (file?.is_lazy_stub) await requestLazyFiles([file.source_index]);
  }

  // ── Register FlatNavigator with diffNav ───────────────────────────────────
  $effect(() => {
    diffNav.register({
      scrollToRow: (rowIdx, align) => {
        if (!scrollEl) return;
        const top = effectiveGeometry.cumulativeOffsets[rowIdx] ?? 0;
        if (align === "center") {
          applyScrollTop(top - viewportHeightPx / 2);
        } else {
          applyScrollTop(top);
        }
      },
      scrollToEdge: (to) => {
        if (!scrollEl) return;
        applyScrollTop(to === "top" ? 0 : scrollEl.scrollHeight);
      },
      scrollAfterCollapse,
      requestFileContent: (src) => requestLazyFiles([src]),
      getModel: () => crossFileModel,
      getFiles: () => files,
    });
    return () => diffNav.unregister();
  });

  // ── Drag-select: frozen geometry + rAF-coalesced window tracking ─────────
  let dragGeometry: EffectiveGeometry | null = $state(null);
  let pendingDragEvent: MouseEvent | null = null;
  let dragRafId: number | null = null;

  function ensureDragGeometry() {
    if (dragGeometry !== null) return;
    dragGeometry = {
      cumulativeOffsets: effectiveGeometry.cumulativeOffsets.slice(),
      totalHeight: effectiveGeometry.totalHeight,
      rowCount: effectiveGeometry.rowCount,
    };
  }

  function anchorFileBounds(): { start: number; end: number } | null {
    const file = diffSel.file;
    if (file === null) return null;
    const start = crossFileModel.fileStartRow.get(file);
    if (start === undefined) return null;
    let end = start;
    while (end < crossFileModel.rows.length && crossFileModel.rows[end].filePath === file) {
      end++;
    }
    return { start, end };
  }

  function dragTargetAtRow(idx: number): { line: number; side: "old" | "new" } | null {
    if (idx < 0 || idx >= crossFileModel.rows.length) return null;
    const row = crossFileModel.rows[idx];
    if (row.filePath !== diffSel.file) return null;

    if (row.type === "content-unified") {
      const file = files.find((f) => f.path === row.filePath);
      const line = file?.hunks[row.hunkIdx]?.lines[row.lineIdx];
      if (!line) return null;
      const side = unifiedLineSide(line);
      if (side !== diffSel.side) return null;
      const ln = line.new_num ?? line.old_num;
      return ln !== null ? { line: ln, side } : null;
    }

    if (row.type === "content-split") {
      if (diffSel.side === null) return null;
      const splitRowsByHunk = crossFileModel.splitRowsByFile.get(row.filePath);
      const splitRow = splitRowsByHunk?.[row.hunkIdx]?.[row.splitRowIdx];
      if (!splitRow) return null;
      const activeSide = diffSel.side === "old" ? splitRow.left : splitRow.right;
      const ln = activeSide ? (activeSide.new_num ?? activeSide.old_num ?? null) : null;
      return ln !== null ? { line: ln, side: diffSel.side } : null;
    }

    return null;
  }

  function lineInfoAtRow(idx: number) {
    if (idx < 0 || idx >= crossFileModel.rows.length) return null;
    const row = crossFileModel.rows[idx];
    if (row.type === "content-unified") {
      const file = files.find((f) => f.path === row.filePath);
      const line = file?.hunks[row.hunkIdx]?.lines[row.lineIdx];
      if (!line) return null;
      return {
        rowIdx: idx,
        rowType: row.type,
        filePath: row.filePath,
        line: line.new_num ?? line.old_num ?? null,
        side: unifiedLineSide(line),
      };
    }
    if (row.type === "content-split") {
      const splitRowsByHunk = crossFileModel.splitRowsByFile.get(row.filePath);
      const splitRow = splitRowsByHunk?.[row.hunkIdx]?.[row.splitRowIdx];
      const left = splitRow?.left ? (splitRow.left.new_num ?? splitRow.left.old_num ?? null) : null;
      const right = splitRow?.right ? (splitRow.right.new_num ?? splitRow.right.old_num ?? null) : null;
      return {
        rowIdx: idx,
        rowType: row.type,
        filePath: row.filePath,
        line: diffSel.side === "old" ? left : right,
        side: diffSel.side,
        left,
        right,
      };
    }
    return {
      rowIdx: idx,
      rowType: row.type,
      filePath: row.filePath,
      line: null,
      side: null,
    };
  }

  function logDragSelection(args: {
    clientY: number;
    contentTop: number;
    rawY: number;
    yPx: number;
    idx: number;
    mouseover: ReturnType<typeof lineInfoAtRow>;
    target: { line: number; side: "old" | "new" } | null;
    hitSource: "dom" | "geometry";
  }) {
    profileLog("drag_select", {
      hit_source: args.hitSource,
      client_y: Math.round(args.clientY),
      content_top: Math.round(args.contentTop),
      raw_y: Math.round(args.rawY),
      y_px: Math.round(args.yPx),
      pointer_row_idx: args.idx,
      mouseover_row_idx: args.mouseover?.rowIdx ?? -1,
      mouseover_row_type: args.mouseover?.rowType ?? "none",
      mouseover_file: args.mouseover?.filePath ?? "none",
      mouseover_line: args.mouseover?.line ?? -1,
      mouseover_side: args.mouseover?.side ?? "none",
      target_line: args.target?.line ?? -1,
      target_side: args.target?.side ?? "none",
      selected_file: diffSel.file ?? "none",
      selected_side: diffSel.side ?? "none",
      selected_start: diffSel.start ?? -1,
      selected_end: diffSel.end ?? -1,
      selected_first: diffSel.start === null || diffSel.end === null ? -1 : diffSel.first(),
      selected_last: diffSel.start === null || diffSel.end === null ? -1 : diffSel.last(),
    });
  }

  function firstSelectableInAnchorFile(bounds: { start: number; end: number }) {
    for (let i = bounds.start; i < bounds.end; i++) {
      const target = dragTargetAtRow(i);
      if (target) return target;
    }
    return null;
  }

  function lastSelectableInAnchorFile(bounds: { start: number; end: number }) {
    for (let i = bounds.end - 1; i >= bounds.start; i--) {
      const target = dragTargetAtRow(i);
      if (target) return target;
    }
    return null;
  }

  function dragTargetForIndex(idx: number) {
    const bounds = anchorFileBounds();
    const anchorIdx = diffSel.startRowIdx;
    if (!bounds || anchorIdx === null) return null;
    if (idx < bounds.start) return firstSelectableInAnchorFile(bounds);
    if (idx >= bounds.end) return lastSelectableInAnchorFile(bounds);
    const row = crossFileModel.rows[idx];
    if (row?.filePath !== diffSel.file) {
      return idx < anchorIdx ? firstSelectableInAnchorFile(bounds) : lastSelectableInAnchorFile(bounds);
    }
    return dragTargetAtRow(idx);
  }

  function rowIndexFromPoint(e: MouseEvent): number | null {
    for (const el of document.elementsFromPoint(e.clientX, e.clientY)) {
      const rowEl = el instanceof HTMLElement ? el.closest<HTMLElement>("[data-row-idx]") : null;
      const raw = rowEl?.dataset.rowIdx;
      if (raw === undefined) continue;
      const idx = Number(raw);
      if (Number.isInteger(idx)) return idx;
    }
    return null;
  }

  function processDragMove(e: MouseEvent) {
    if (!diffSel.dragging || !hscrollEl) return;
    if (!diffSel.exceededDragSlop(e)) return;
    ensureDragGeometry();
    const geom = dragGeometry!;
    const rect = hscrollEl.getBoundingClientRect();
    const rawY = rowOffsetFromContentTopY(e.clientY, rect.top);
    const yPx = Math.max(0, Math.min(rawY, geom.totalHeight - 1));
    const domIdx = rowIndexFromPoint(e);
    const idx = domIdx ?? rowIndexAtOffset(geom, yPx);
    const mouseover = lineInfoAtRow(idx);
    const target = dragTargetForIndex(idx);
    if (target) diffSel.extend(target.line, target.side);
    logDragSelection({
      clientY: e.clientY,
      contentTop: rect.top,
      rawY,
      yPx,
      idx,
      mouseover,
      target,
      hitSource: domIdx === null ? "geometry" : "dom",
    });
  }

  function scheduleDragMove(e: MouseEvent) {
    pendingDragEvent = e;
    if (dragRafId !== null) return;
    dragRafId = requestAnimationFrame(() => {
      dragRafId = null;
      const ev = pendingDragEvent;
      pendingDragEvent = null;
      if (ev) processDragMove(ev);
    });
  }

  function clearDragSession() {
    if (dragRafId !== null) {
      cancelAnimationFrame(dragRafId);
      dragRafId = null;
    }
    pendingDragEvent = null;
    dragGeometry = null;
  }

  // ── Measured row heights → effectiveGeometry overlay ─────────────────────
  let heightRo: ResizeObserver | null = null;
  $effect(() => {
    if (!scrollEl) return;
    void vw.start;
    void vw.end;
    heightRo?.disconnect();
    heightRo = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const el = entry.target as HTMLElement;
        const identity = el.dataset.rowIdentity;
        if (!identity) continue;
        onHeightChange(identity, Math.round(entry.contentRect.height));
      }
    });
    scrollEl.querySelectorAll<HTMLElement>("[data-row-identity]").forEach((el) => {
      heightRo!.observe(el);
    });
    return () => heightRo?.disconnect();
  });

  // ── DEV height validator (Step F) ────────────────────────────────────────
  let devRo: ResizeObserver | null = null;
  $effect(() => {
    if (!import.meta.env.DEV || !scrollEl) return;
    if (globalThis.localStorage?.getItem("erDevHeightProbe") !== "1") return;
    void vw.start;
    void vw.end;
    devRo?.disconnect();
    devRo = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const el = entry.target as HTMLElement;
        const identity = el.dataset.rowIdentity;
        if (!identity) continue;
        const actual = Math.round(entry.contentRect.height);
        const row = crossFileModel.rows.find((r) => r.identity === identity);
        if (!row || FIXED_HEIGHT_ROW_TYPES.has(row.type)) continue;
        const expected = overlayHeights.get(identity) ?? row.height;
        if (Math.abs(actual - expected) > 1) {
          console.error(`[er-dev] height mismatch: ${identity} expected=${expected} actual=${actual}`);
          profileLogRateLimited("dev_height_fix", {
            identity,
            expected,
            actual,
          });
        }
      }
    });
    if (!scrollEl || !devRo) return () => devRo?.disconnect();
    scrollEl.querySelectorAll<HTMLElement>("[data-row-identity]").forEach((el) => {
      devRo!.observe(el);
    });
    return () => devRo?.disconnect();
  });

  onMount(() => {
    const onMove = (e: MouseEvent) => {
      if (diffSel.dragging) scheduleDragMove(e);
    };
    const onUp = () => {
      if (diffSel.dragging) clearDragSession();
      diffSel.finish();
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);

    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) viewportHeightPx = entry.contentRect.height;
    });
    if (scrollEl) {
      ro.observe(scrollEl);
      viewportHeightPx = scrollEl.clientHeight;
      scrollTopLivePx = scrollEl.scrollTop;
    }
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      ro.disconnect();
    };
  });

  // ── Row data helpers ──────────────────────────────────────────────────────
  function getUnifiedLine(row: Extract<CrossFileFlatRow, { type: "content-unified" }>) {
    _syntaxRevision;
    const file = files.find((f) => f.path === row.filePath);
    return file?.hunks[row.hunkIdx]?.lines[row.lineIdx] ?? null;
  }

  function getUnifiedPartner(row: Extract<CrossFileFlatRow, { type: "content-unified" }>) {
    _syntaxRevision;
    const file = files.find((f) => f.path === row.filePath);
    const hunk = file?.hunks[row.hunkIdx];
    if (!hunk) return null;
    return computeUnifiedPairs(hunk)[row.lineIdx]?.partner ?? null;
  }

  function getSplitRow(row: Extract<CrossFileFlatRow, { type: "content-split" }>) {
    _syntaxRevision;
    const file = files.find((f) => f.path === row.filePath);
    const hunk = file?.hunks[row.hunkIdx];
    if (!hunk) return null;
    return splitRows(hunk.lines)[row.splitRowIdx] ?? null;
  }

  function getThread(threadId: string) {
    return threadMap.get(threadId) ?? null;
  }

  function getFinding(findingId: string) {
    return annotationIndex.findingMap.get(findingId) ?? null;
  }
</script>

<div class="flex-1 flex flex-col min-w-0 overflow-hidden relative">
  <!-- Top bar -->
  {#if treeHidden || files.length > 0}
    <div class="h-10 px-4 border-b border-hairline bg-ink-870 flex items-center gap-3 shrink-0 text-muted">
      {#if treeHidden}
        <button
          class="p-1 hover:text-fg-2 hover:bg-hover rounded shrink-0"
          onclick={() => app.togglePanel("tree")}
          title="Show file tree"
          aria-label="Show file tree"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z"/></svg>
        </button>
      {/if}
      <span class="mono text-xs text-fg-3">{files.length} {files.length === 1 ? "file" : "files"}</span>
      <div class="ml-auto flex items-center gap-1">
        {#if tourAvailable && !tourFresh}
          <!-- New changes landed since the guide was generated — offer a re-run. -->
          <button
            class="flex items-center gap-1 h-[22px] px-2 mr-1 rounded text-[11px] font-medium border border-risk-med/40 text-risk-med hover:bg-risk-med/10 transition-colors shrink-0"
            onclick={() => { app.showToast("info", "Regenerating guide…"); void app.cmd("generate_tour"); }}
            title="The diff changed since this guide was generated — regenerate it"
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M23 4v6h-6"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>
            Re-run guide
          </button>
        {/if}
        {#if tourAvailable}
          <div role="tablist" class="flex items-center bg-ink-800 border border-hairline rounded-md p-0.5 mr-1 shrink-0">
            <button
              role="tab"
              aria-selected={mode !== "tour"}
              onclick={exitGuideToDiff}
              class="h-[22px] px-2.5 rounded text-[11px] font-medium transition-colors {mode !== 'tour' ? 'bg-ink-650 text-fg cursor-default' : 'text-muted hover:text-fg-2'}"
            >
              Diff
            </button>
            <button
              role="tab"
              aria-selected={mode === "tour"}
              onclick={() => { if (mode !== "tour") void app.cmd("set_mode", { mode: "tour" }); }}
              class="flex items-center gap-1 h-[22px] px-2.5 rounded text-[11px] font-medium transition-colors {mode === 'tour' ? 'bg-ink-650 text-fg cursor-default' : 'text-muted hover:text-fg-2'}"
            >
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 18l6-6-6-6"/><circle cx="4" cy="12" r="1.5"/></svg>
              Guide
            </button>
          </div>
        {/if}
        <button
          type="button"
          class="p-1 text-fg-3 hover:bg-hover rounded flex items-center"
          onclick={collapseAllDiffFiles}
          title="Collapse all files"
          aria-label="Collapse all file diffs"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="m18 15-6-6-6 6"/>
          </svg>
        </button>
        <button
          type="button"
          class="p-1 text-fg-3 hover:bg-hover rounded flex items-center"
          onclick={() => diffFileCollapse.expandAll()}
          title="Expand all files"
          aria-label="Expand all file diffs"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="m6 9 6 6 6-6"/>
          </svg>
        </button>
        <div class="relative">
          <button
            class="px-2 py-1 text-xs text-fg-3 hover:bg-hover rounded flex items-center"
            onclick={() => (settingsOpen = !settingsOpen)}
            title="View settings"
            aria-label="View settings"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="12" cy="12" r="3"/>
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
            </svg>
          </button>
          {#if settingsOpen}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="fixed inset-0 z-40" onclick={() => (settingsOpen = false)}></div>
            <div class="absolute right-0 top-full mt-1 z-50 bg-ink-800 border border-ink-500 rounded shadow-xl w-52 py-1">
              <div class="px-3 pt-2 pb-1 text-[11px] uppercase tracking-wide text-fg-3">Layout</div>
              <button
                class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
                onclick={() => { app.setDiffViewMode("unified"); settingsOpen = false; }}
              >
                <span class="w-3 inline-flex items-center justify-center">
                  {#if viewMode === "unified"}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M5 13l4 4L19 7"/></svg>
                  {/if}
                </span>
                Unified
              </button>
              <button
                class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
                onclick={() => { app.setDiffViewMode("split"); settingsOpen = false; }}
              >
                <span class="w-3 inline-flex items-center justify-center">
                  {#if viewMode === "split"}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M5 13l4 4L19 7"/></svg>
                  {/if}
                </span>
                Split
              </button>
              <div class="border-t border-ink-600 my-1"></div>
              <div class="px-3 pt-2 pb-1 text-[11px] uppercase tracking-wide text-fg-3">Annotations</div>
              <button
                class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
                onclick={() => app.setCommentVisibility({ hideComments: !app.commentVisibility.hideComments })}
              >
                <span class="w-3 inline-flex items-center justify-center">
                  {#if !app.commentVisibility.hideComments}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M5 13l4 4L19 7"/></svg>
                  {/if}
                </span>
                Comments
              </button>
              <button
                class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
                onclick={() => app.setCommentVisibility({ hideFindings: !app.commentVisibility.hideFindings })}
              >
                <span class="w-3 inline-flex items-center justify-center">
                  {#if !app.commentVisibility.hideFindings}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M5 13l4 4L19 7"/></svg>
                  {/if}
                </span>
                Findings
              </button>
              <button
                class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
                onclick={() => app.setCommentVisibility({ hideQuestions: !app.commentVisibility.hideQuestions })}
              >
                <span class="w-3 inline-flex items-center justify-center">
                  {#if !app.commentVisibility.hideQuestions}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M5 13l4 4L19 7"/></svg>
                  {/if}
                </span>
                Questions
              </button>
            </div>
          {/if}
        </div>
        <button
          class="px-2 py-1 text-xs text-fg-3 hover:bg-hover rounded"
          onclick={async () => {
            const res = await invoke<{ kind: string; target: string }>("open_source");
            if (res.kind === "needs_checkout") app.showToast("info", "Create editable worktree to open locally");
          }}
        >
          Open source
        </button>
      </div>
    </div>
  {/if}

  <!-- D11 three-layer scroll DOM. Wrapped so the reference-highlight overview
       ruler and usages popover can overlay the scroll viewport (instead of
       scrolling away with the content). -->
  <div class="flex-1 min-h-0 relative flex flex-col">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={scrollEl}
    class="vscroll flex-1 mono text-[13px] leading-[1.55] relative {diffSel.dragging ? 'select-none' : ''}"
    onscroll={onScroll}
  >
    {#if !snapshot}
      <div class="flex items-center justify-center h-full text-muted">Loading…</div>
    {:else if files.length === 0 && snapshot.bg_loading?.tab_diff}
      <!-- First diff load of a stub tab runs on a background thread — show a
           loading state instead of flashing "No changes". -->
      <div class="flex items-center justify-center h-full text-muted text-sm gap-2">
        <span class="inline-block w-1.5 h-1.5 rounded-full bg-accent animate-pulse"></span>
        Loading diff…
      </div>
    {:else if files.length === 0}
      <div class="flex items-center justify-center h-full text-muted text-sm">No changes</div>
    {:else}
      <!-- Sticky file path overlay: hides when real file-header is in viewport top band.
           In Guide mode it's inset past the pillar rail lane so it pins over the
           diff column (column 2) without covering the rail. -->
      <StickyFileHeader
        row={visibleFileHeaderRow}
        hidden={stickyHeaderHidden}
        offsetLeftPx={tourActive ? RAIL_W : 0}
      />

      {#if tourActive}
        <!-- Column 1: pillar rail lane. Each pillar block aligns to its diffs in
             column 2 and pins its rail (sticky) while that pillar is on screen. -->
        <div
          class="pillar-rail-lane"
          style="position:absolute;top:{STICKY_HEADER_PX}px;left:0;width:{RAIL_W}px;height:{effectiveGeometry.totalHeight}px;z-index:20;border-right:1px solid var(--color-hairline);"
        >
          {#each pillarSpans as span (span.info.pillarId)}
            <div style="position:absolute;left:0;right:0;top:{span.topPx}px;height:{span.heightPx}px;">
              <div
                style="position:sticky;top:0;"
                use:measureHeight={(px) => setRailHeight(span.info.pillarId, px)}
              >
                <PillarRail
                  info={span.info}
                  fileRows={pillarFileRows.get(span.info.pillarId) ?? []}
                  {relatedRows}
                  selectedPath={visibleFilePath}
                />
              </div>
            </div>
          {/each}
        </div>
      {/if}

      <!-- X-scroll surface: full-height absolute-positioned band (column 2 in Guide) -->
      <div
        bind:this={hscrollEl}
        class="hscroll"
        style="height:{effectiveGeometry.totalHeight + bottomPadPx}px;overflow-x:auto;overflow-y:hidden;position:relative;{tourActive
          ? `margin-left:${RAIL_W}px;width:calc(100% - ${RAIL_W}px);`
          : 'width:100%;'}"
      >
        <div
          class="band"
          style="position:absolute;top:{vw.paddingTop}px;left:0;right:0;min-width:max-content"
        >
          {#each windowedRows as row, localIdx (row.identity)}
            {@const rowIdx = vw.start + localIdx}
            {#if row.type === "file-header"}
              <FileHeaderRow
                {row}
                pointerEventsNone={stickyHeaderClicksOverlay && row.filePath === visibleFilePath}
              />
            {:else if row.type === "hunk-header"}
              <HunkHeaderRow {row} />
            {:else if row.type === "content-fold"}
              <FoldLineRow {row} />
            {:else if row.type === "content-unified"}
              {@const line = getUnifiedLine(row)}
              {@const partner = getUnifiedPartner(row)}
              {#if line}
                <UnifiedRow
                  {row}
                  {line}
                  {partner}
                  filePath={row.filePath}
                  {rowIdx}
                  {annotationIndex}
                  commentVisibility={app.commentVisibility}
                />
              {/if}
            {:else if row.type === "content-split"}
              {@const splitRow = getSplitRow(row)}
              {#if splitRow}
                <SplitContentRow
                  {row}
                  {splitRow}
                  filePath={row.filePath}
                  {rowIdx}
                  {annotationIndex}
                  commentVisibility={app.commentVisibility}
                />
              {/if}
            {:else if row.type === "compacted-stub"}
              <CompactedStubRow {row} />
            {:else if row.type === "lazy-stub"}
              <LazyStubRow {row} />
            {:else if row.type === "no-changes"}
              <NoChangesRow {row} />
            {:else if row.type === "inline-thread" || row.type === "fallback-thread"}
              {@const thread = getThread(row.threadId)}
              {#if thread}
                <ThreadRow {row} {thread} />
              {/if}
            {:else if row.type === "inline-finding" || row.type === "fallback-finding"}
              {@const finding = getFinding(row.findingId)}
              {@const thread = finding?.thread_id ? getThread(finding.thread_id) : null}
              {#if finding}
                <FindingRow {row} {finding} {thread} />
              {/if}
            {/if}
            {#if tourActive && pillarPadByRowIdentity.get(row.identity)}
              <!-- Guide mode: pad the pillar's last row down to the rail height so
                   the next pillar's files start below the (taller) rail. -->
              <div style="height:{pillarPadByRowIdentity.get(row.identity)}px"></div>
            {/if}
          {/each}
        </div>
      </div>

      {#if diffSel.composerOpen}
        <DiffComposer topPx={composerTopPx} {viewMode} />
      {/if}
    {/if}
  </div>

  {#if refHighlight.searchOpen}
    <DiffSearchBar
      total={usageResult.total}
      capped={usageResult.capped}
      activeIdx={refHighlight.searchActiveIdx}
      onNavigate={navigateSearch}
    />
  {/if}
  {#if usageMarks.length > 0}
    <ReferenceRuler
      marks={usageMarks}
      onJump={jumpToUsage}
      getPreview={previewForMarkRow}
      query={refHighlight.identifier ?? ""}
      matchOpts={refHighlight.matchOptions}
    />
  {/if}
  {#if refHighlight.popoverOpen && refHighlight.identifier !== null}
    <ReferenceUsagesPopover
      identifier={refHighlight.identifier}
      usages={usageLinesAll}
      anchor={refHighlight.popoverAnchor}
      onJump={jumpToUsageLine}
      getContext={(u) => contextForUsage(u, POPOVER_CONTEXT_LINES)}
    />
  {/if}
  </div>

  {#if showGoBackToComment}
    <div class="pointer-events-none absolute inset-x-0 bottom-3 z-30 flex justify-center">
      <ComposerScrollBack onGoBack={scrollComposerIntoView} />
    </div>
  {/if}
</div>

<style>
  .vscroll {
    overflow-y: auto;
    overflow-x: hidden;
    scroll-padding-bottom: var(--shell-bottom-chrome, 32px);
  }
</style>
