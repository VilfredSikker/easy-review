<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app, type DiffViewMode } from "$lib/stores/app.svelte";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { diffScroll } from "$lib/stores/diffScroll.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import DiffComposer from "./DiffComposer.svelte";
  import FileHeaderRow from "./diff-rows/FileHeaderRow.svelte";
  import HunkHeaderRow from "./diff-rows/HunkHeaderRow.svelte";
  import UnifiedRow from "./diff-rows/UnifiedRow.svelte";
  import SplitContentRow from "./diff-rows/SplitContentRow.svelte";
  import CompactedStubRow from "./diff-rows/CompactedStubRow.svelte";
  import LazyStubRow from "./diff-rows/LazyStubRow.svelte";
  import NoChangesRow from "./diff-rows/NoChangesRow.svelte";
  import ThreadRow from "./diff-rows/ThreadRow.svelte";
  import FindingRow from "./diff-rows/FindingRow.svelte";
  import StickyFileHeader from "./diff-rows/StickyFileHeader.svelte";
  import {
    getCrossFileModel,
    type CrossFileModel,
    type CrossFileFlatRow,
  } from "$lib/diffRenderModel";
  import {
    windowFromScrollVariable,
    rowIndexAtOffset,
    type EffectiveGeometry,
  } from "$lib/virtualWindow";
  import { buildAnnotationIndex } from "$lib/diffAnnotations";
  import { makeScrollThrottle } from "$lib/scrollThrottle";
  import { highlightCache, type HighlightResult } from "$lib/highlightCache";
  import { buildTree, flattenForNav } from "$lib/treeFromPaths";
  import type { AppSnapshot, FileSnapshot, SpanSnapshot } from "$lib/types";

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

  const snapshotKey = $derived(
    snapshot ? `${snapshot.active_tab}:${snapshot.mode}:${snapshot.base}:${snapshot.branch}` : mode,
  );

  const annotationIndex = $derived.by(() =>
    buildAnnotationIndex(
      snapshot?.ai ?? { threads: [], findings: [] } as any,
      files,
      mode,
      app.commentVisibility,
    ),
  );
  const threadMap = $derived(annotationIndex.threadMap);

  // ── Cross-file model ───────────────────────────────────────────────────────
  const crossFileModel = $derived(
    getCrossFileModel({
      files,
      viewMode,
      mode,
      annotationIndex,
      commentVisibility: app.commentVisibility,
      snapshotKey,
    }),
  );

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
  let scrollTopPx = $state(0);
  let viewportHeightPx = $state(0);

  const _updateScrollTop = makeScrollThrottle((top) => { scrollTopPx = top; });

  let lastViewKey: string | null = null;
  let scrollSaveTimer: ReturnType<typeof setTimeout> | null = null;
  let treeFollowTimer: ReturnType<typeof setTimeout> | null = null;

  function onScroll() {
    if (!scrollEl) return;
    const top = scrollEl.scrollTop;
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
    lastViewKey = snapshotKey;
    tick().then(() => { if (scrollEl) scrollEl.scrollTop = top; });
  });

  // ── Virtual window ────────────────────────────────────────────────────────
  const OVERSCAN = 5;
  const vw = $derived(
    windowFromScrollVariable(
      effectiveGeometry.cumulativeOffsets,
      effectiveGeometry.totalHeight,
      scrollTopPx,
      viewportHeightPx,
      OVERSCAN,
    ),
  );
  const windowedRows = $derived(crossFileModel.rows.slice(vw.start, vw.end));

  // ── Visible file (for sticky header) ─────────────────────────────────────
  const visibleFilePath = $derived.by(() => {
    const idx = rowIndexAtOffset(effectiveGeometry, scrollTopPx);
    if (idx < 0 || idx >= crossFileModel.rows.length) return null;
    return crossFileModel.rows[idx].filePath ?? null;
  });

  // Hide sticky overlay when the real file-header row is in [scrollTopPx, scrollTopPx+40).
  const stickyHeaderHidden = $derived.by(() => {
    if (!visibleFilePath) return false;
    const startRow = crossFileModel.fileStartRow.get(visibleFilePath);
    if (startRow === undefined) return false;
    const headerTop = effectiveGeometry.cumulativeOffsets[startRow] ?? 0;
    return headerTop >= scrollTopPx && headerTop < scrollTopPx + 40;
  });

  // ── Selection clear on file change ───────────────────────────────────────
  $effect(() => {
    const selectedFile = snapshot?.files[snapshot.selected_file];
    if (selectedFile && diffSel.file !== selectedFile.path) {
      diffSel.clear();
      diffSel.file = selectedFile.path;
    }
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
      // Wake the highlight $effect — its previous run early-exited on
      // `is_lazy_stub`, so it didn't subscribe to the deep line-level
      // properties that just changed. Bump the counter to force a re-run.
      _highlightGeneration++;
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

  // ── Highlight effect ──────────────────────────────────────────────────────
  const HIGHLIGHT_THEME = "OneHalfDark";
  // Reactive counter — incrementing re-triggers the effect so newly-unblocked
  // files get picked up when the 4-concurrent cap frees up.
  let _highlightGeneration = $state(0);
  const _highlightInFlight = new Set<string>();

  function applyHunkSpans(
    snapFile: FileSnapshot,
    hunks: { hunk_index: number; lines: SpanSnapshot[][] }[],
  ): void {
    const spansByHunkIdx = new Map<number, SpanSnapshot[][]>();
    for (const hh of hunks) spansByHunkIdx.set(hh.hunk_index, hh.lines);
    snapFile.hunks = snapFile.hunks.map((hunk, hIdx) => {
      const spans = spansByHunkIdx.get(hIdx);
      if (!spans) return hunk;
      return {
        ...hunk,
        lines: hunk.lines.map((line, lIdx) =>
          spans[lIdx] !== undefined ? { ...line, spans: spans[lIdx] } : line,
        ),
      };
    });
  }

  $effect(() => {
    _highlightGeneration; // reactive dep: re-runs when a request completes
    const rows = windowedRows;
    const visiblePaths = new Set(rows.map((r) => r.filePath));
    for (const filePath of visiblePaths) {
      const file = files.find((f) => f.path === filePath);
      if (!file || file.is_lazy_stub || file.hunks.length === 0) continue;
      const firstContentLine = file.hunks[0]?.lines.find((l) => l.kind !== "fold");
      if (!firstContentLine || firstContentLine.spans !== undefined) continue;
      const cacheKey = highlightCache.key(file.path, file.cache_key, HIGHLIGHT_THEME);
      // Cache hit: backend poll wiped spans from the snapshot, but we still
      // have the highlight result cached. Re-apply it instead of re-requesting.
      const cachedHunks = highlightCache.get(cacheKey);
      if (cachedHunks) {
        const snapFile = app.snapshot?.files?.find((f) => f.path === file.path);
        if (snapFile) applyHunkSpans(snapFile, cachedHunks);
        continue;
      }
      if (_highlightInFlight.has(file.path)) continue;
      if (_highlightInFlight.size >= 4) continue;
      _highlightInFlight.add(file.path);
      invoke<HighlightResult>("highlight_file", {
        filePath: file.path,
        cacheKey: file.cache_key,
        syntaxTheme: HIGHLIGHT_THEME,
      })
        .then((result) => {
          _highlightInFlight.delete(file.path);
          _highlightGeneration++; // wake effect for remaining files
          if (!result || !app.snapshot) return;
          const snapFile = app.snapshot.files?.find((f) => f.path === file.path);
          if (!snapFile || result.cache_key !== snapFile.cache_key) return;
          highlightCache.set(cacheKey, result.hunks);
          applyHunkSpans(snapFile, result.hunks);
        })
        .catch(() => {
          _highlightInFlight.delete(file.path);
          _highlightGeneration++;
        });
    }
  });

  // ── Composer position ─────────────────────────────────────────────────────
  const composerTopPx = $derived.by(() => {
    if (!diffSel.active || diffSel.file === null || diffSel.end === null) return undefined;
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
          lineNum = ln ? (ln.new_num ?? ln.old_num ?? null) : null;
        } else {
          const splitRowsByHunk = crossFileModel.splitRowsByFile.get(diffSel.file);
          const sr = splitRowsByHunk?.[row.hunkIdx]?.[row.splitRowIdx];
          const activeSide = sr?.right ?? sr?.left;
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

  // ── Scroll to keep selection + composer both visible when composer opens ──
  $effect(() => {
    const top = composerTopPx;
    if (top === undefined || !scrollEl) return;
    const LINE_H = 20;
    const viewBottom = scrollTopPx + viewportHeightPx;
    // If the last selected line (just above `top`) is below the viewport midpoint
    // or the composer is off-screen, scroll so the selected line sits at ~25% from
    // the top and the composer is visible below it.
    const selectedLineTop = top - LINE_H;
    if (selectedLineTop > scrollTopPx + viewportHeightPx * 0.5 || top > viewBottom) {
      scrollEl.scrollTop = Math.max(0, selectedLineTop - Math.floor(viewportHeightPx * 0.25));
    }
  });

  // ── Register FlatNavigator with diffNav ───────────────────────────────────
  $effect(() => {
    diffNav.register({
      scrollToRow: (rowIdx, align) => {
        if (!scrollEl) return;
        const top = effectiveGeometry.cumulativeOffsets[rowIdx] ?? 0;
        if (align === "center") {
          scrollEl.scrollTop = Math.max(0, top - viewportHeightPx / 2);
        } else {
          scrollEl.scrollTop = top;
        }
      },
      scrollToEdge: (to) => {
        if (!scrollEl) return;
        scrollEl.scrollTop = to === "top" ? 0 : scrollEl.scrollHeight;
      },
      requestFileContent: (src) => requestLazyFile(src),
      getModel: () => crossFileModel,
      getFiles: () => files,
    });
    return () => diffNav.unregister();
  });

  // ── Drag-select: container onmousemove ────────────────────────────────────
  function onMouseMove(e: MouseEvent) {
    if (!diffSel.dragging || !scrollEl) return;
    const rect = scrollEl.getBoundingClientRect();
    const yPx = e.clientY - rect.top + scrollTopPx;
    const idx = rowIndexAtOffset(effectiveGeometry, yPx);
    if (idx < 0 || idx >= crossFileModel.rows.length) return;
    const row = crossFileModel.rows[idx];
    if (row.filePath !== diffSel.file) return;
    if (row.type === "content-unified") {
      const file = files.find((f) => f.path === row.filePath);
      const line = file?.hunks[row.hunkIdx]?.lines[row.lineIdx];
      const ln = line ? (line.new_num ?? line.old_num) : null;
      if (ln !== null) diffSel.extend(ln);
    } else if (row.type === "content-split") {
      const xPct = (e.clientX - rect.left) / rect.width;
      const side = xPct < 0.5 ? "old" : "new";
      const model = crossFileModel;
      const splitRows = model.splitRowsByFile.get(row.filePath);
      const splitRow = splitRows?.[row.hunkIdx]?.[row.splitRowIdx];
      if (!splitRow) return;
      const ln = side === "old"
        ? (splitRow.left ? (splitRow.left.new_num ?? splitRow.left.old_num) : null)
        : (splitRow.right ? (splitRow.right.new_num ?? splitRow.right.old_num) : null);
      if (ln !== null && diffSel.side === side) diffSel.extend(ln);
    }
  }

  // ── DEV height validator (Step F) ────────────────────────────────────────
  let devRo: ResizeObserver | null = null;
  $effect(() => {
    if (!import.meta.env.DEV || !scrollEl) return;
    devRo?.disconnect();
    devRo = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const el = entry.target as HTMLElement;
        const identity = el.dataset.rowIdentity;
        if (!identity) continue;
        const actual = Math.round(entry.contentRect.height);
        const row = crossFileModel.rows.find((r) => r.identity === identity);
        const expected = row ? (overlayHeights.get(identity) ?? row.height) : null;
        if (expected !== null && Math.abs(actual - expected) > 1) {
          console.error(`[er-dev] height mismatch: ${identity} expected=${expected} actual=${actual}`);
          onHeightChange(identity, actual);
        }
      }
    });
    // Re-observe windowed rows each render tick.
    const reattach = () => {
      if (!scrollEl || !devRo) return;
      scrollEl.querySelectorAll<HTMLElement>("[data-row-identity]").forEach((el) => {
        devRo!.observe(el);
      });
    };
    reattach();
    return () => devRo?.disconnect();
  });

  onMount(() => {
    const onUp = () => diffSel.finish();
    window.addEventListener("mouseup", onUp);

    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) viewportHeightPx = entry.contentRect.height;
    });
    if (scrollEl) {
      ro.observe(scrollEl);
      viewportHeightPx = scrollEl.clientHeight;
    }
    return () => {
      window.removeEventListener("mouseup", onUp);
      ro.disconnect();
    };
  });

  // ── Row data helpers ──────────────────────────────────────────────────────
  function getUnifiedLine(row: Extract<CrossFileFlatRow, { type: "content-unified" }>) {
    const file = files.find((f) => f.path === row.filePath);
    return file?.hunks[row.hunkIdx]?.lines[row.lineIdx] ?? null;
  }

  function getUnifiedPartner(row: Extract<CrossFileFlatRow, { type: "content-unified" }>) {
    const pairs = crossFileModel.unifiedPairsByFile.get(row.filePath)?.[row.hunkIdx];
    return pairs?.[row.lineIdx]?.partner ?? null;
  }

  function getSplitRow(row: Extract<CrossFileFlatRow, { type: "content-split" }>) {
    return crossFileModel.splitRowsByFile.get(row.filePath)?.[row.hunkIdx]?.[row.splitRowIdx] ?? null;
  }

  function getThread(threadId: string) {
    return threadMap.get(threadId) ?? null;
  }

  function getFinding(findingId: string) {
    return annotationIndex.findingMap.get(findingId) ?? null;
  }
</script>

<div class="flex-1 flex flex-col min-w-0 overflow-hidden">
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
    onmousemove={onMouseMove}
    onmouseleave={() => diffSel.finish()}
  >
    {#if !snapshot}
      <div class="flex items-center justify-center h-full text-muted">Loading…</div>
    {:else if files.length === 0}
      <div class="flex items-center justify-center h-full text-muted text-sm">No changes</div>
    {:else}
      <!-- Sticky file path overlay: hides when real file-header is in viewport top band -->
      <StickyFileHeader filePath={visibleFilePath} hidden={stickyHeaderHidden} />

      <!-- X-scroll surface: full-height absolute-positioned band -->
      <div
        class="hscroll"
        style="height:{effectiveGeometry.totalHeight}px;overflow-x:auto;overflow-y:visible;position:relative;width:100%"
      >
        <div
          class="band"
          style="position:absolute;top:{vw.paddingTop}px;left:0;right:0;min-width:max-content"
        >
          {#each windowedRows as row (row.identity)}
            {#if row.type === "file-header"}
              <FileHeaderRow {row} />
            {:else if row.type === "hunk-header"}
              <HunkHeaderRow {row} />
            {:else if row.type === "content-unified"}
              {@const line = getUnifiedLine(row)}
              {@const partner = getUnifiedPartner(row)}
              {#if line}
                <UnifiedRow {row} {line} {partner} filePath={row.filePath} />
              {/if}
            {:else if row.type === "content-split"}
              {@const splitRow = getSplitRow(row)}
              {#if splitRow}
                <SplitContentRow {row} {splitRow} filePath={row.filePath} />
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

      {#if diffSel.active}
        <DiffComposer topPx={composerTopPx} />
      {/if}
    {/if}
  </div>
</div>

<style>
  .vscroll {
    overflow-y: auto;
    overflow-x: hidden;
  }
</style>
