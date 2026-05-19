<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app, type DiffViewMode } from "$lib/stores/app.svelte";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { diffScroll } from "$lib/stores/diffScroll.svelte";
  import InlineThread from "./InlineThread.svelte";
  import InlineFinding from "./InlineFinding.svelte";
  import DiffComposer from "./DiffComposer.svelte";
  import { splitRows } from "$lib/splitRows";
  import { wordDiff } from "$lib/wordDiff";
  import type { FileSnapshot, FlatFinding, LineSnapshot, ThreadSnapshot } from "$lib/types";

  /** Flatten a line's spans into plain text — used as input to word-diff. */
  function lineText(line: LineSnapshot): string {
    let out = "";
    for (const s of line.spans) out += s.text;
    return out;
  }

  /**
   * For unified mode: walk `lines` and pair each del with the add at the same
   * offset within a `del* add*` block. Returns the partner text for each
   * line that is part of a balanced modify pair (and `null` for unpaired
   * adds/dels and context).
   */
  function unifiedPairs(lines: LineSnapshot[]): (string | null)[] {
    const pairs: (string | null)[] = new Array(lines.length).fill(null);
    let i = 0;
    while (i < lines.length) {
      if (lines[i].kind !== "del") {
        i++;
        continue;
      }
      const delStart = i;
      while (i < lines.length && lines[i].kind === "del") i++;
      const addStart = i;
      while (i < lines.length && lines[i].kind === "add") i++;
      const delCount = addStart - delStart;
      const addCount = i - addStart;
      const pairCount = Math.min(delCount, addCount);
      for (let k = 0; k < pairCount; k++) {
        pairs[delStart + k] = lineText(lines[addStart + k]);
        pairs[addStart + k] = lineText(lines[delStart + k]);
      }
    }
    return pairs;
  }

  interface Props {
    /** Storybook/testing override that bypasses the app store's view mode. */
    viewModeOverride?: DiffViewMode | null;
  }

  const { viewModeOverride = null }: Props = $props();

  /** Leading whitespace count, used for hanging-indent on wrapped lines. */
  function leadingWS(line: LineSnapshot): number {
    const t = lineText(line);
    let n = 0;
    while (n < t.length && (t[n] === " " || t[n] === "\t")) n++;
    return n;
  }

  /** Inline style giving wrapped continuations a hanging indent that lands
   *  one column past the code's natural indent — same pattern as GitHub's
   *  wrapped diff so continuations are visually distinct from real indentation
   *  changes. 0.75rem matches the `px-3` horizontal padding; the +2 accounts
   *  for the +/-/space marker plus one extra column of hang. */
  function hangingIndent(line: LineSnapshot): string {
    const cols = leadingWS(line) + 2;
    return `padding-left: calc(0.75rem + ${cols}ch); text-indent: -${cols}ch;`;
  }

  function lineClass(kind: string) {
    if (kind === "add") return "diff-add";
    if (kind === "del") return "diff-del";
    return "";
  }

  function gutterClass(kind: string) {
    if (kind === "add") return "diff-add";
    if (kind === "del") return "diff-del";
    return "";
  }

  // Syntect's base16-ocean palette emits some low-contrast token colors
  // (notably punctuation/comments) that become hard to read on add/del rows.
  // Remap only known dim colors to brighter equivalents.
  const spanColorRemap: Record<string, string> = {
    "#4f5b66": "#a7b1ba",
    "#343d46": "#a7b1ba",
    "#65737e": "#a7b1ba",
    "#6b6b6b": "#a7b1ba",
    "#5e5e5e": "#a7b1ba",
    "#99c794": "#8fd7a8",
    "#a3be8c": "#8fd7a8",
  };

  function remapSpanColor(color: string): string {
    if (!color) return color;
    return spanColorRemap[color.toLowerCase()] ?? color;
  }

  const snapshot = $derived(app.snapshot);
  const files = $derived(snapshot?.files ?? []);
  const treeHidden = $derived(!snapshot?.panels.tree);
  const viewMode = $derived<DiffViewMode>(viewModeOverride ?? app.diffViewMode);
  const compactLines = $derived(app.compactLines);
  const mode = $derived(snapshot?.mode ?? "branch");

  let settingsOpen = $state(false);

  const allFindings = $derived(snapshot?.ai.findings ?? []);

  /** Branch mode: match hunk_index when set. Other modes: line number only. */
  function findingMatchesHunk(f: FlatFinding, hunkIndex: number, diffMode: string): boolean {
    if (diffMode !== "branch") return true;
    return f.hunk_index === null || f.hunk_index === hunkIndex;
  }

  /** Whether a finding is owned by this hunk (for footer / fallback placement). */
  function findingBelongsToHunk(
    f: FlatFinding,
    filePath: string,
    hunkIndex: number,
    hunk: { new_start: number; new_count: number },
  ): boolean {
    if (f.file !== filePath) return false;
    if (f.hunk_index !== null) return f.hunk_index === hunkIndex;
    if (f.line !== null) {
      return f.line >= hunk.new_start && f.line < hunk.new_start + hunk.new_count;
    }
    return mode === "branch" ? false : f.hunk_index === hunkIndex;
  }

  /** Line-anchored findings for a diff row (mirrors AiState::findings_for_line). */
  function findingsForLine(
    filePath: string,
    hunkIndex: number,
    targetLine: number,
    hunkLines: LineSnapshot[],
    skipDelDuplicate: boolean,
  ): FlatFinding[] {
    return allFindings.filter((f) => {
      if (f.file !== filePath || f.line !== targetLine) return false;
      if (!findingMatchesHunk(f, hunkIndex, mode)) return false;
      if (skipDelDuplicate && hunkLines.some((l) => l.new_num === targetLine)) return false;
      return true;
    });
  }

  /** True when a line-anchored finding would render on some row in this hunk. */
  function findingRendersInline(
    f: FlatFinding,
    filePath: string,
    hunkIndex: number,
    hunkLines: LineSnapshot[],
  ): boolean {
    if (f.file !== filePath || f.line === null) return false;
    if (!findingMatchesHunk(f, hunkIndex, mode)) return false;
    for (const line of hunkLines) {
      const ln = lineNum(line);
      if (ln !== f.line) continue;
      if (line.kind === "del" && hunkLines.some((l) => l.new_num === f.line)) continue;
      return true;
    }
    return false;
  }

  /** Hunk-level findings (no line anchor), matching TUI hunk footer. */
  function hunkLevelFindings(
    filePath: string,
    hunkIndex: number,
    hunk: { new_start: number; new_count: number },
  ): FlatFinding[] {
    return allFindings.filter(
      (f) => f.file === filePath && f.line === null && findingBelongsToHunk(f, filePath, hunkIndex, hunk),
    );
  }

  /** Line-anchored findings whose target line is not rendered in this hunk. */
  function fallbackFindings(
    filePath: string,
    hunkIndex: number,
    hunk: { new_start: number; new_count: number },
    hunkLines: LineSnapshot[],
  ): FlatFinding[] {
    return allFindings.filter(
      (f) =>
        f.file === filePath &&
        f.line !== null &&
        findingBelongsToHunk(f, filePath, hunkIndex, hunk) &&
        !findingRendersInline(f, filePath, hunkIndex, hunkLines),
    );
  }

  /** Split row: match left or right line numbers (same as inline threads). */
  function findingsForSplitRow(
    filePath: string,
    hunkIndex: number,
    leftLn: number | null,
    rightLn: number | null,
    hunkLines: LineSnapshot[],
  ): FlatFinding[] {
    const out: FlatFinding[] = [];
    const seen = new Set<string>();
    for (const ln of [rightLn, leftLn]) {
      if (ln === null) continue;
      for (const f of findingsForLine(filePath, hunkIndex, ln, hunkLines, false)) {
        if (seen.has(f.id)) continue;
        seen.add(f.id);
        out.push(f);
      }
    }
    return out;
  }

  /** Map of thread id → ThreadSnapshot for O(1) lookup. */
  const threadMap = $derived.by(() => {
    const map = new Map<string, ThreadSnapshot>();
    if (!snapshot) return map;
    for (const t of snapshot.ai.threads) map.set(t.id, t);
    return map;
  });

  /** Set of thread IDs that are owned by a finding (rendered inside its card). */
  const findingThreadIds = $derived.by(() => {
    const ids = new Set<string>();
    if (!snapshot) return ids;
    for (const f of snapshot.ai.findings) {
      if (f.thread_id) ids.add(f.thread_id);
    }
    return ids;
  });

  /** Clear stale selection when the focused file changes (still keyed off selected_file). */
  $effect(() => {
    const selectedFile = snapshot?.files[snapshot.selected_file];
    if (selectedFile && diffSel.file !== selectedFile.path) {
      diffSel.clear();
      diffSel.file = selectedFile.path;
    }
  });

  let scrollEl: HTMLDivElement | null = $state(null);
  let observer: IntersectionObserver | null = null;
  let mountObserver: IntersectionObserver | null = null;
  let windowObserver: IntersectionObserver | null = null;
  /** Map of file-path → most recent intersection ratio. */
  const intersectionRatios = new Map<string, number>();

  /**
   * Paths whose body is currently mounted. A file's body renders if (a) it is
   * within ± 1 viewport of the scroll area (tracked by windowObserver), or (b)
   * it is the focused file (`snapshot.selected_file`). Out-of-window files
   * render a height-estimated stub instead.
   */
  let inView = $state<Set<string>>(new Set());
  /** Per-path measured body height, captured after first render. Used to keep
   *  the placeholder height accurate when the body is unmounted, so scroll
   *  position is preserved within a few px. */
  const measuredHeights = new Map<string, number>();

  /** Rough up-front height estimate so the initial stub reserves space. ~21px
   *  per line, +22px per hunk header, with a 60px floor for empty/no-change
   *  files. Matches mono leading-[1.55] @ 13px. */
  function estimateHeight(file: FileSnapshot): number {
    return file.hunks.reduce((acc, h) => acc + 22 + h.lines.length * 21, 0) || 60;
  }

  function placeholderHeight(file: FileSnapshot): number {
    return measuredHeights.get(file.path) ?? estimateHeight(file);
  }

  const selectedPath = $derived(snapshot?.files[snapshot.selected_file]?.path ?? null);
  const viewKey = $derived(
    snapshot ? `${snapshot.active_tab}:${snapshot.mode}:${snapshot.base}:${snapshot.branch}` : mode,
  );

  function visibleThreads(threads: ThreadSnapshot[]): ThreadSnapshot[] {
    const visibility = app.commentVisibility;
    if (visibility.hideAll) return [];
    return threads.filter(
      (thread) =>
        !findingThreadIds.has(thread.id) &&
        !(visibility.hideResolved && thread.resolved) &&
        !(visibility.hideOutdated && thread.stale),
    );
  }

  function refreshCurrentFile() {
    let best: { path: string; ratio: number } | null = null;
    for (const [path, ratio] of intersectionRatios) {
      if (!best || ratio > best.ratio) best = { path, ratio };
    }
    diffScroll.currentFilePath = best && best.ratio > 0 ? best.path : null;
  }

  /** Attach IntersectionObservers to every file section. Re-runs when files change. */
  $effect(() => {
    // Re-run when the file set changes so we re-observe newly-rendered sections.
    files;
    viewKey;
    if (!scrollEl) return;
    // Tear down old observers and reset state — paths may have changed.
    observer?.disconnect();
    mountObserver?.disconnect();
    windowObserver?.disconnect();
    intersectionRatios.clear();
    diffScroll.currentFilePath = null;
    inView = new Set();

    observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const path = (entry.target as HTMLElement).dataset.filePath;
          if (!path) continue;
          intersectionRatios.set(path, entry.intersectionRatio);
        }
        refreshCurrentFile();
      },
      {
        root: scrollEl,
        // Track section visibility — header crossing the top is what we care about.
        rootMargin: "0px 0px -70% 0px",
        threshold: [0, 0.01, 0.5, 1],
      },
    );

    // Asymmetric windowed rendering: mount when within ±1 viewport, unmount
    // only when beyond ±2 viewports. This prevents thrash when the user scrolls
    // slowly near a boundary — the element stays mounted through small reversals.
    // Two observers implement the hysteresis: the mount observer adds to inView,
    // the unmount observer removes from it.
    mountObserver = new IntersectionObserver(
      (entries) => {
        let changed = false;
        const next = new Set(inView);
        for (const entry of entries) {
          const path = (entry.target as HTMLElement).dataset.filePath;
          if (!path) continue;
          if (entry.isIntersecting && !next.has(path)) { next.add(path); changed = true; }
        }
        if (changed) inView = next;
      },
      { root: scrollEl, rootMargin: "100% 0px 100% 0px", threshold: 0 },
    );
    windowObserver = new IntersectionObserver(
      (entries) => {
        let changed = false;
        const next = new Set(inView);
        for (const entry of entries) {
          const path = (entry.target as HTMLElement).dataset.filePath;
          if (!path) continue;
          if (!entry.isIntersecting && next.has(path)) { next.delete(path); changed = true; }
        }
        if (changed) inView = next;
      },
      { root: scrollEl, rootMargin: "200% 0px 200% 0px", threshold: 0 },
    );

    // Defer until DOM has rendered the sections.
    tick().then(() => {
      if (!scrollEl) return;
      scrollEl.querySelectorAll<HTMLElement>("section[data-file-path]").forEach((el) => {
        observer?.observe(el);
        mountObserver?.observe(el);
        windowObserver?.observe(el);
      });
    });
  });

  /** Measure mounted file bodies and cache their height. Used so the stub that
   *  replaces them on unmount preserves scroll position. */
  $effect(() => {
    inView;
    if (!scrollEl) return;
    tick().then(() => {
      if (!scrollEl) return;
      scrollEl.querySelectorAll<HTMLElement>("[data-file-body]").forEach((el) => {
        const path = el.dataset.fileBody;
        if (!path) return;
        const h = el.offsetHeight;
        if (h > 0) measuredHeights.set(path, h);
      });
    });
  });

  /** Persist scroll position per mode + restore when mode changes. */
  let lastMode: string | null = null;
  let scrollSaveTimer: ReturnType<typeof setTimeout> | null = null;

  function onScroll() {
    if (!scrollEl) return;
    if (scrollSaveTimer) clearTimeout(scrollSaveTimer);
    const top = scrollEl.scrollTop;
    const curKey = viewKey;
    scrollSaveTimer = setTimeout(() => {
      diffScroll.setScrollTop(curKey, top);
    }, 150);
  }

  $effect(() => {
    if (!scrollEl) return;
    if (lastMode === viewKey) return;
    // View changed — restore scroll position (or 0 if never seen).
    const top = diffScroll.getScrollTop(viewKey);
    lastMode = viewKey;
    tick().then(() => {
      if (scrollEl) scrollEl.scrollTop = top;
    });
  });

  // Global mouseup so a drag that ends outside the scroll area still finishes cleanly.
  onMount(() => {
    const onUp = () => diffSel.finish();
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mouseup", onUp);
      observer?.disconnect();
      mountObserver?.disconnect();
      windowObserver?.disconnect();
    };
  });

  /**
   * The displayed line number is always the new-num (or old-num for deletions).
   * Used for both drag-select tracking and the gutter display.
   */
  function lineNum(line: { new_num: number | null; old_num: number | null }): number | null {
    return line.new_num ?? line.old_num;
  }

  /** True when a side cell should render as selected — respects the captured side in split. */
  function selectedOnSide(ln: number | null, side: "old" | "new"): boolean {
    if (ln === null) return false;
    if (!diffSel.sel(ln)) return false;
    if (viewMode === "split" && diffSel.side !== null && diffSel.side !== side) return false;
    return true;
  }

  function fileIdx(path: string): number {
    return files.findIndex((f) => f.path === path);
  }

  function expandCompacted(path: string) {
    const file = files.find((f) => f.path === path);
    if (!file) return;
    // Focus the file, then expand via the existing toggle_compacted command.
    app.cmd("select_file", { idx: file.source_index });
    app.cmd("toggle_compacted");
  }
</script>

<div class="flex-1 flex flex-col min-w-0 overflow-hidden">
  <!-- Top thin bar: tree-show toggle + view-mode controls. The per-file headers
       inside each <section> are sticky and carry the file path / counts. -->
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
              <div class="border-t border-ink-500 my-1"></div>
              <button
                class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
                onclick={() => app.toggleCompactLines()}
              >
                <span class="w-3 inline-flex items-center justify-center">
                  {#if compactLines}
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M5 13l4 4L19 7"/></svg>
                  {/if}
                </span>
                Compact line height
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

  <!-- Continuous diff scroll area. Each file is its own <section> with a
       sticky per-file header. Position-sticky inside the section means each
       header sticks until the next section pushes it out — exactly the
       GitHub/Linear pattern. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={scrollEl}
    class="flex-1 overflow-y-auto mono text-[13px] {compactLines ? 'leading-[1.25]' : 'leading-[1.55]'} relative {diffSel.dragging ? 'select-none' : ''}"
    onmouseleave={() => diffSel.finish()}
    onscroll={onScroll}
  >
    {#if !snapshot}
      <div class="flex items-center justify-center h-full text-muted">Loading…</div>
    {:else if files.length === 0}
      <div class="flex items-center justify-center h-full text-muted text-sm">No changes</div>
    {:else}
      {#each files as file (file.path)}
        {@const bodyMounted = file.compacted || inView.has(file.path) || file.path === selectedPath}
        <section
          id={`file-${file.path}`}
          data-file-path={file.path}
          class="border-b border-hairline"
        >
          <!-- Per-file sticky header (anchor target for FileTree clicks). -->
          <div class="sticky top-0 z-10 h-10 px-4 border-b border-hairline bg-ink-870 flex items-center gap-3 shrink-0 text-muted">
            <span class="mono text-sm text-fg-2 truncate">{file.path}</span>
            <span class="mono text-xs text-add-fg shrink-0">+{file.additions}</span>
            <span class="mono text-xs text-del-fg shrink-0">−{file.deletions}</span>
            <div class="ml-auto flex items-center gap-1">
              <button
                class="px-2 py-1 text-xs flex items-center gap-1 hover:bg-hover rounded {file.reviewed ? 'text-add-fg' : 'text-fg-3'}"
                onclick={() => {
                  const idx = fileIdx(file.path);
                  if (idx < 0) return;
                  app.cmd(file.reviewed ? "unmark_reviewed" : "mark_reviewed", { fileIdx: idx });
                }}
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 13l4 4L19 7"/></svg>
                {file.reviewed ? "Unmark" : "Mark reviewed"}
              </button>
            </div>
          </div>

          {#if !bodyMounted}
            <!-- Virtualization stub: out-of-window file body. The height comes
                 from a prior measurement when available, otherwise an estimate.
                 Preserves scroll position when the body un-mounts. -->
            <div
              data-file-stub={file.path}
              style="height: {placeholderHeight(file)}px"
              aria-hidden="true"
            ></div>
          {:else}
          <div data-file-body={file.path} class="w-full">
          {#if file.compacted}
            <!-- Compacted placeholder: single clickable row that expands inline. -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="px-4 py-3 text-muted text-sm italic cursor-pointer hover:bg-hover"
              onclick={() => expandCompacted(file.path)}
            >
              File compacted — click or press Enter to expand
            </div>
          {:else if file.hunks.length === 0}
            <div class="px-4 py-3 text-muted text-sm">{file.is_lazy_stub ? "Loading content…" : "No changes"}</div>
          {:else if viewMode === "unified"}
            {#each file.hunks as hunk, hunkIndex}
              <div class="px-4 py-1 text-muted bg-surface border-b border-hairline text-[12px]">
                {hunk.header}
              </div>
              {@const pairs = unifiedPairs(hunk.lines)}
              {#each hunk.lines as line, lineIdx}
                {@const ln = lineNum(line)}
                {@const partner = pairs[lineIdx]}
                {@const wdU = partner !== null
                  ? (line.kind === "del"
                      ? wordDiff(lineText(line), partner)
                      : line.kind === "add"
                        ? wordDiff(partner, lineText(line))
                        : null)
                  : null}
                {@const wdSpans = wdU ? (line.kind === "del" ? wdU.old : wdU.new) : null}
                {@const wdBg = line.kind === "del" ? "bg-del-fg/30" : "bg-add-fg/30"}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="grid grid-cols-[40px_40px_minmax(0,1fr)] diff-row {ln !== null && diffSel.file === file.path && diffSel.sel(ln) ? 'is-selected' : ''}"
                  onmouseenter={() => { if (ln !== null && diffSel.file === file.path) diffSel.extend(ln); }}
                >
                  <div class="text-right pr-2 gutter {gutterClass(line.kind)}">
                    {line.old_num ?? ""}
                  </div>
                  <div class="text-right pr-2 gutter {gutterClass(line.kind)}">
                    {line.new_num ?? ""}
                    {#if ln !== null && line.kind !== "fold"}
                      <button
                        class="add-comment-btn"
                        onmousedown={(e) => diffSel.begin(ln, e.shiftKey, e, file.path)}
                      >+</button>
                    {/if}
                  </div>
                  <div
                    class="pr-3 whitespace-pre-wrap break-all {lineClass(line.kind)}"
                    style={hangingIndent(line)}
                  >
                    {#if line.kind === "add"}
                      <span class="text-add-fg">+</span>
                    {:else if line.kind === "del"}
                      <span class="text-del-fg">-</span>
                    {:else}
                      <span>&nbsp;</span>
                    {/if}
                    {#if wdSpans}
                      {#each wdSpans as wspan}
                        {#if wspan.changed}
                          <span class={wdBg}>{wspan.text}</span>
                        {:else}
                          {wspan.text}
                        {/if}
                      {/each}
                    {:else}
                      {#each line.spans as span}
                        {#if span.color}
                          <span style="color: {remapSpanColor(span.color)}">{span.text}</span>
                        {:else}
                          {span.text}
                        {/if}
                      {/each}
                    {/if}
                  </div>
                </div>
                {#each visibleThreads(hunk.threads).filter((t) => {
                    if (ln === null || t.line !== ln) return false;
                    // Don't show on del lines when an add line in this hunk has the same line number
                    if (line.kind === "del" && hunk.lines.some((l: LineSnapshot) => l.new_num === t.line)) return false;
                    return true;
                  }) as thread (thread.id)}
                  <InlineThread {thread} hunk_idx={hunkIndex} />
                {/each}
                {#if ln !== null}
                  {#each findingsForLine(file.path, hunkIndex, ln, hunk.lines, line.kind === "del" && hunk.lines.some((l: LineSnapshot) => l.new_num === ln)) as finding (finding.id)}
                    <InlineFinding {finding} thread={finding.thread_id ? (threadMap.get(finding.thread_id) ?? null) : null} />
                  {/each}
                {/if}
              {/each}

              <!-- Hunk-level AI findings (no line anchor) -->
              {#each hunkLevelFindings(file.path, hunkIndex, hunk) as finding (finding.id)}
                <InlineFinding {finding} thread={finding.thread_id ? (threadMap.get(finding.thread_id) ?? null) : null} />
              {/each}

              <!-- Fallback: line-anchored findings whose target line wasn't rendered -->
              {#each fallbackFindings(file.path, hunkIndex, hunk, hunk.lines) as finding (finding.id)}
                <InlineFinding {finding} thread={finding.thread_id ? (threadMap.get(finding.thread_id) ?? null) : null} />
              {/each}

              <!-- Fallback: threads whose target line wasn't rendered in this hunk -->
              {#each visibleThreads(hunk.threads).filter((t) => !hunk.lines.some((l) => lineNum(l) === t.line)) as thread (thread.id)}
                <InlineThread {thread} hunk_idx={hunkIndex} />
              {/each}
            {/each}
          {:else}
            <!-- Split (side-by-side) view -->
            {#each file.hunks as hunk, hunkIndex}
              <div class="px-4 py-1 text-muted bg-surface border-b border-hairline text-[12px]">
                {hunk.header}
              </div>
              {@const rows = splitRows(hunk.lines)}
              {#each rows as row, rowIndex (rowIndex)}
                {@const left = row.left}
                {@const right = row.right}
                {@const leftLn = left ? lineNum(left) : null}
                {@const rightLn = right ? lineNum(right) : null}
                {@const isModifyPair = !!(left && right && left.kind === "del" && right.kind === "add")}
                {@const wd = isModifyPair ? wordDiff(lineText(left!), lineText(right!)) : null}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="grid grid-cols-[40px_minmax(0,1fr)_40px_minmax(0,1fr)] diff-row">
                  <div
                    class="text-right pr-2 gutter {left ? gutterClass(left.kind) : 'diff-empty'} {selectedOnSide(leftLn, 'old') && diffSel.file === file.path ? 'is-selected' : ''}"
                    onmouseenter={() => { if (leftLn !== null && diffSel.file === file.path && diffSel.side === "old") diffSel.extend(leftLn); }}
                  >
                    {left?.old_num ?? ""}
                    {#if left && leftLn !== null && left.kind !== "fold"}
                      <button
                        class="add-comment-btn"
                        onmousedown={(e) => diffSel.begin(leftLn, e.shiftKey, e, file.path, "old")}
                      >+</button>
                    {/if}
                  </div>
                  <div
                    class="pr-3 whitespace-pre-wrap break-all {left ? lineClass(left.kind) : 'diff-empty'} {selectedOnSide(leftLn, 'old') && diffSel.file === file.path ? 'is-selected' : ''}"
                    style={left ? hangingIndent(left) : "padding-left: 0.75rem"}
                    onmouseenter={() => { if (leftLn !== null && diffSel.file === file.path && diffSel.side === "old") diffSel.extend(leftLn); }}
                  >
                    {#if left}
                      {#if left.kind === "del"}
                        <span class="text-del-fg">-</span>
                      {:else if left.kind === "add"}
                        <span class="text-add-fg">+</span>
                      {:else}
                        <span>&nbsp;</span>
                      {/if}
                      {#if wd}
                        {#each wd.old as wspan}
                          {#if wspan.changed}
                            <span class="bg-del-fg/30">{wspan.text}</span>
                          {:else}
                            {wspan.text}
                          {/if}
                        {/each}
                      {:else}
                        {#each left.spans as span}
                          {#if span.color}
                            <span style="color: {remapSpanColor(span.color)}">{span.text}</span>
                          {:else}
                            {span.text}
                          {/if}
                        {/each}
                      {/if}
                    {:else}
                      <span>&nbsp;</span>
                    {/if}
                  </div>
                  <div
                    class="text-right pr-2 gutter {right ? gutterClass(right.kind) : 'diff-empty'} {selectedOnSide(rightLn, 'new') && diffSel.file === file.path ? 'is-selected' : ''}"
                    onmouseenter={() => { if (rightLn !== null && diffSel.file === file.path && diffSel.side === "new") diffSel.extend(rightLn); }}
                  >
                    {right?.new_num ?? ""}
                    {#if right && rightLn !== null && right.kind !== "fold"}
                      <button
                        class="add-comment-btn"
                        onmousedown={(e) => diffSel.begin(rightLn, e.shiftKey, e, file.path, "new")}
                      >+</button>
                    {/if}
                  </div>
                  <div
                    class="pr-3 whitespace-pre-wrap break-all {right ? lineClass(right.kind) : 'diff-empty'} {selectedOnSide(rightLn, 'new') && diffSel.file === file.path ? 'is-selected' : ''}"
                    style={right ? hangingIndent(right) : "padding-left: 0.75rem"}
                    onmouseenter={() => { if (rightLn !== null && diffSel.file === file.path && diffSel.side === "new") diffSel.extend(rightLn); }}
                  >
                    {#if right}
                      {#if right.kind === "add"}
                        <span class="text-add-fg">+</span>
                      {:else if right.kind === "del"}
                        <span class="text-del-fg">-</span>
                      {:else}
                        <span>&nbsp;</span>
                      {/if}
                      {#if wd}
                        {#each wd.new as wspan}
                          {#if wspan.changed}
                            <span class="bg-add-fg/30">{wspan.text}</span>
                          {:else}
                            {wspan.text}
                          {/if}
                        {/each}
                      {:else}
                        {#each right.spans as span}
                          {#if span.color}
                            <span style="color: {remapSpanColor(span.color)}">{span.text}</span>
                          {:else}
                            {span.text}
                          {/if}
                        {/each}
                      {/if}
                    {:else}
                      <span>&nbsp;</span>
                    {/if}
                  </div>
                </div>

                {#each visibleThreads(hunk.threads).filter((t) => (leftLn !== null && t.line === leftLn) || (rightLn !== null && t.line === rightLn)) as thread (thread.id)}
                  <div class="col-span-4">
                    <InlineThread {thread} hunk_idx={hunkIndex} />
                  </div>
                {/each}
                {#each findingsForSplitRow(file.path, hunkIndex, leftLn, rightLn, hunk.lines) as finding (finding.id)}
                  <div class="col-span-4">
                    <InlineFinding {finding} thread={finding.thread_id ? (threadMap.get(finding.thread_id) ?? null) : null} />
                  </div>
                {/each}
              {/each}

              {#each hunkLevelFindings(file.path, hunkIndex, hunk) as finding (finding.id)}
                <div class="col-span-4">
                  <InlineFinding {finding} thread={finding.thread_id ? (threadMap.get(finding.thread_id) ?? null) : null} />
                </div>
              {/each}

              {#each fallbackFindings(file.path, hunkIndex, hunk, hunk.lines) as finding (finding.id)}
                <div class="col-span-4">
                  <InlineFinding {finding} thread={finding.thread_id ? (threadMap.get(finding.thread_id) ?? null) : null} />
                </div>
              {/each}

              {#each visibleThreads(hunk.threads).filter((t) => !hunk.lines.some((l: LineSnapshot) => lineNum(l) === t.line)) as thread (thread.id)}
                <div class="col-span-4">
                  <InlineThread {thread} hunk_idx={hunkIndex} />
                </div>
              {/each}
            {/each}
          {/if}
          </div>
          {/if}
        </section>
      {/each}
    {/if}

    {#if diffSel.active}
      <DiffComposer />
    {/if}
  </div>
</div>

<style>
  .diff-add { background: var(--color-add-bg); }
  .diff-del { background: var(--color-del-bg); }
  .diff-empty { background: rgba(255, 255, 255, 0.02); }
</style>
