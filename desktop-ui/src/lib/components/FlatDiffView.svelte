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
  import {
    applyCollapsedFiles,
    computeUnifiedPairs,
    getCrossFileModel,
    type CrossFileModel,
    type CrossFileFlatRow,
  } from "$lib/diffRenderModel";
  import { diffFileCollapse } from "$lib/stores/diffFileCollapse.svelte";
  import { splitRows } from "$lib/splitRows";
  import {
    windowFromScrollVariable,
    rowIndexAtOffset,
    rowOffsetFromContentTopY,
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
    const orderedPaths = flattenForNav(buildTree(all));
    const byPath = new Map(all.map((f) => [f.path, f]));
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
      snapshot?.ai ?? { threads: [], findings: [] } as any,
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

  // ── D10 measured-height overlay ───────────────────────────────────────────
  let overlayHeights = $state(new Map<string, number>());
  let overlaySerial = $state(0);

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
  const effectiveGeometry = $derived.by<EffectiveGeometry>(() => {
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
    _spansAppliedKeys.clear();
    lastViewKey = snapshotKey;
    tick().then(() => { if (scrollEl) scrollEl.scrollTop = top; });
  });

  // ── Virtual window ────────────────────────────────────────────────────────
  const OVERSCAN = 5;
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
    ),
  );
  const windowedRows = $derived(crossFileModel.rows.slice(vw.start, vw.end));

  // ── Visible file (for sticky header) ─────────────────────────────────────
  const visibleFilePath = $derived.by(() => {
    const idx = rowIndexAtOffset(effectiveGeometry, rowScrollTopPx);
    if (idx < 0 || idx >= crossFileModel.rows.length) return null;
    return crossFileModel.rows[idx].filePath ?? null;
  });

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
    return headerTop >= rowScrollTopPx && headerTop < rowScrollTopPx + STICKY_HEADER_PX;
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
  const REQUEST_FILE_CONCURRENCY = 2;

  async function requestLazyFile(sourceIndex: number): Promise<void> {
    if (_requestingFiles.has(sourceIndex)) return;
    _requestingFiles.add(sourceIndex);
    const reqSnap = app.snapshot;
    const reqTab = reqSnap?.active_tab;
    const reqMode = reqSnap?.mode;
    const reqBase = reqSnap?.base;
    const reqBranch = reqSnap?.branch;
    try {
      const snap = await invoke<AppSnapshot>("request_file_content", { sourceIndex });
      if (!snap || !app.snapshot) return;
      if (
        app.snapshot.active_tab !== reqTab ||
        app.snapshot.mode !== reqMode ||
        app.snapshot.base !== reqBase ||
        app.snapshot.branch !== reqBranch
      ) return;
      const oldFile = app.snapshot.files.find((f) => f.source_index === sourceIndex);
      const newFile = snap.files.find((f) => f.source_index === sourceIndex);
      if (!oldFile || !newFile) return;
      oldFile.hunks = newFile.hunks;
      oldFile.is_lazy_stub = newFile.is_lazy_stub;
      oldFile.compacted = newFile.compacted;
      oldFile.additions = newFile.additions;
      oldFile.deletions = newFile.deletions;
      oldFile.cache_key = newFile.cache_key;
      evictSpanKeysForPath(oldFile.path);
      scheduleHighlightDrain();
    } finally {
      _requestingFiles.delete(sourceIndex);
    }
  }

  $effect(() => {
    const rows = windowedRows;
    for (const row of rows) {
      if (row.type !== "lazy-stub") continue;
      if (_requestingFiles.size >= REQUEST_FILE_CONCURRENCY) break;
      if (_requestingFiles.has(row.sourceIndex)) continue;
      requestLazyFile(row.sourceIndex);
    }
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
        if (!cacheExhausted) {
          const changed = untrack(() => tryApplyHighlightSpans(snapFile, cachedHunks, spanKey));
          if (changed) cacheApplied += 1;
          else cacheApplySkipped += 1;
          continue;
        }
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
    if (!scrollToFileHeader(target.path)) return;
    const file = files.find((f) => f.path === target.path);
    if (file?.is_lazy_stub) await requestLazyFile(file.source_index);
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
      requestFileContent: (src) => requestLazyFile(src),
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

  <!-- D11 three-layer scroll DOM -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={scrollEl}
    class="vscroll flex-1 mono text-[13px] leading-[1.55] relative {diffSel.dragging ? 'select-none' : ''}"
    onscroll={onScroll}
  >
    {#if !snapshot}
      <div class="flex items-center justify-center h-full text-muted">Loading…</div>
    {:else if files.length === 0}
      <div class="flex items-center justify-center h-full text-muted text-sm">No changes</div>
    {:else}
      <!-- Sticky file path overlay: hides when real file-header is in viewport top band -->
      <StickyFileHeader row={visibleFileHeaderRow} hidden={stickyHeaderHidden} />

      <!-- X-scroll surface: full-height absolute-positioned band -->
      <div
        bind:this={hscrollEl}
        class="hscroll"
        style="height:{effectiveGeometry.totalHeight}px;overflow-x:auto;overflow-y:visible;position:relative;width:100%"
      >
        <div
          class="band"
          style="position:absolute;top:{vw.paddingTop}px;left:0;right:0;min-width:max-content"
        >
          {#each windowedRows as row, localIdx (row.identity)}
            {@const rowIdx = vw.start + localIdx}
            {#if row.type === "file-header"}
              <FileHeaderRow {row} />
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
          {/each}
        </div>
      </div>

      {#if diffSel.composerOpen}
        <DiffComposer topPx={composerTopPx} {viewMode} />
      {/if}
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
  }
</style>
