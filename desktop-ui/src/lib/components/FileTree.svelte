<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { buildTree, filesByPathMap, resolveTreeFile, visibleTree } from "$lib/treeFromPaths";
  import ScopeSelector from "./ScopeSelector.svelte";
  import FileRailRow from "./FileRailRow.svelte";
  import { onDestroy } from "svelte";
  import { windowFromScroll } from "$lib/virtualWindow";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { diffScroll } from "$lib/stores/diffScroll.svelte";
  import { fileTreeCollapse } from "$lib/stores/fileTreeCollapse.svelte";
  import type { FileSnapshot } from "$lib/types";
  import { hiddenFindingCounts, toggledPath, toggledPaths } from "$lib/fileTreeModel";
  import { moveHighlight, suggestionFooter, suggestionLabel, visibleSuggestions } from "$lib/filterSuggestions";
  import { findingsVisibility } from "$lib/stores/findingsVisibility.svelte";

  interface Props {
    /** When true, render narrow icon-only rail (mock lines 414–421). */
    collapsed?: boolean;
    /** Checkbox multi-select for AI file picker modals. */
    pickerMode?: boolean;
    /** Fill parent flex area (no fixed sidebar width). */
    embedded?: boolean;
    /** Override file list (picker uses full diff paths, not filtered snapshot). */
    files?: FileSnapshot[];
    /** Checked paths when `pickerMode` is true. */
    selectedPaths?: Set<string>;
    onSelectedPathsChange?: (paths: Set<string>) => void;
    /** Enter in picker mode (e.g. run review). */
    onPickerEnter?: () => void;
  }
  const {
    collapsed = false,
    pickerMode = false,
    embedded = false,
    files: filesOverride,
    selectedPaths,
    onSelectedPathsChange,
    onPickerEnter,
  }: Props = $props();

  const snapshot = $derived(app.snapshot);

  /** Accurate tooltip for the annotation badge — breaks out comments / questions
   *  / findings so a question never reads as a "comment". */
  /// Findings the confidence gate is holding back, per file.
  ///
  /// The backend's `finding_count` is every active finding; the gate is the
  /// reader's, so only the client can say how many it is hiding. A count that
  /// silently shrinks is how a filter stops being trusted.
  const hiddenFindings = $derived(
    hiddenFindingCounts(app.snapshot?.ai, (fallback) => findingsVisibility.minTrust(fallback)),
  );

  function annotationTitle(file: FileSnapshot): string {
    const parts: string[] = [];
    if (file.comment_count > 0) {
      parts.push(`${file.comment_count} comment${file.comment_count !== 1 ? "s" : ""}`);
    }
    if (file.question_count > 0) {
      parts.push(`${file.question_count} question${file.question_count !== 1 ? "s" : ""}`);
    }
    if (file.finding_count > 0) {
      parts.push(`${file.finding_count} finding${file.finding_count !== 1 ? "s" : ""}`);
    }
    const hidden = hiddenFindings.get(file.path) ?? 0;
    if (hidden > 0) parts.push(`${hidden} hidden by the confidence gate`);
    return parts.join(" · ");
  }

  // ── Filter input state ────────────────────────────────────────────────────
  let filterDraft = $state("");
  let inputFocused = $state(false);

  const sourceFiles = $derived(
    pickerMode && filesOverride !== undefined ? filesOverride : (snapshot?.files ?? []),
  );

  function localFilterFiles(list: FileSnapshot[], query: string): FileSnapshot[] {
    const q = query.trim().toLowerCase();
    if (!q) return list;
    return list.filter((f) => f.path.toLowerCase().includes(q));
  }

  const files = $derived(
    pickerMode ? localFilterFiles(sourceFiles, filterDraft) : sourceFiles,
  );
  const filesByPath = $derived(filesByPathMap(files));
  const tree = $derived(buildTree(files));
  const displayTree = $derived(visibleTree(tree, fileTreeCollapse.collapsed));
  const selectedFile = $derived(
    pickerMode || !snapshot ? null : sourceFiles[snapshot.selected_file],
  );
  let pickerFocusIdx = $state(0);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let inputEl: HTMLInputElement | null = $state(null);
  const DEBOUNCE_MS = 180;

  $effect(() => {
    if (pickerMode) return;
    const backendValue = snapshot?.filter ?? "";
    if (!inputFocused || filterDraft === backendValue) {
      filterDraft = backendValue;
    }
  });

  function clearTimer() {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
  }

  function applyFilter(v: string) {
    clearTimer();
    const trimmed = v.trim();
    if (trimmed) app.cmd("set_filter", { query: trimmed });
    else app.cmd("clear_filter");
  }

  /** Quick filters listed under the box for the current draft. */
  const suggestions = $derived(
    pickerMode ? [] : visibleSuggestions(snapshot?.filter_suggestions ?? [], filterDraft),
  );
  /** Row the arrow keys have moved to; -1 while the caret is in the input. */
  let highlight = $state(-1);
  let suggestionsEl: HTMLDivElement | null = $state(null);

  $effect(() => {
    if (highlight < 0 || !suggestionsEl) return;
    suggestionsEl
      .querySelector(`[data-sugg-idx="${highlight}"]`)
      ?.scrollIntoView({ block: "nearest" });
  });

  function onFilterInput(e: Event) {
    highlight = -1;
    filterDraft = (e.target as HTMLInputElement).value;
    if (pickerMode) return;
    clearTimer();
    const v = filterDraft;
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      applyFilter(v);
    }, DEBOUNCE_MS);
  }

  // Blur hides the list after a delay so a suggestion's mousedown lands first.
  // Refocusing inside that window must cancel it, or the list vanishes while
  // the input still has focus and the arrows move a highlight nobody can see.
  let blurTimer: ReturnType<typeof setTimeout> | null = null;

  function onFilterFocus() {
    if (blurTimer !== null) clearTimeout(blurTimer);
    blurTimer = null;
    inputFocused = true;
    highlight = -1;
  }

  function onFilterBlur() {
    blurTimer = setTimeout(() => {
      blurTimer = null;
      inputFocused = false;
    }, 150);
  }

  function onFilterKeydown(e: KeyboardEvent) {
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && inputFocused && suggestions.length > 0) {
      e.preventDefault();
      highlight = moveHighlight(highlight, suggestions.length, e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (pickerMode) return;
      const picked = highlight >= 0 ? suggestions[highlight] : undefined;
      highlight = -1;
      if (picked) pickSuggestion(picked.expr);
      else applyFilter(filterDraft);
      inputEl?.blur();
    } else if (e.key === "Escape" && highlight >= 0) {
      // First Escape leaves the list; the next one clears the filter.
      e.preventDefault();
      highlight = -1;
    } else if (e.key === "Escape") {
      e.preventDefault();
      filterDraft = "";
      if (pickerMode) return;
      clearTimer();
      app.cmd("clear_filter");
      inputEl?.blur();
    }
  }

  function pickSuggestion(expr: string) {
    highlight = -1;
    filterDraft = expr;
    applyFilter(expr);
  }

  onDestroy(() => {
    clearTimer();
    if (blurTimer !== null) clearTimeout(blurTimer);
  });

  // ── Virtualization ────────────────────────────────────────────────────────
  const VIRTUALIZE_THRESHOLD = 1000;
  const ROW_HEIGHT = 28;
  let listEl: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(0);

  $effect(() => {
    const el = listEl;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      viewportHeight = el.clientHeight;
    });
    ro.observe(el);
    viewportHeight = el.clientHeight;
    return () => ro.disconnect();
  });

  const shouldVirtualize = $derived(displayTree.length >= VIRTUALIZE_THRESHOLD);
  const vw = $derived(
    shouldVirtualize
      ? windowFromScroll(displayTree.length, ROW_HEIGHT, scrollTop, viewportHeight)
      : { start: 0, end: displayTree.length, paddingTop: 0, paddingBottom: 0 },
  );
  const visibleNodes = $derived(displayTree.slice(vw.start, vw.end));

  const viewportPath = $derived(diffScroll.currentFilePath);

  function jumpToFile(path: string, sourceIdx: number) {
    fileTreeCollapse.expandAncestorsOf(path);
    diffNav.scrollToFile(path);
    app.cmd("select_file", { idx: sourceIdx });
  }

  function toggleSelection(path: string) {
    onSelectedPathsChange?.(toggledPath(selectedPaths, path));
  }

  function isPathSelected(path: string): boolean {
    return selectedPaths?.has(path) ?? false;
  }

  /** All file paths under `folderPath` (this folder and nested subfolders). */
  function filePathsUnderFolder(folderPath: string): string[] {
    const prefix = `${folderPath}/`;
    return files.filter((f) => f.path.startsWith(prefix)).map((f) => f.path);
  }

  type FolderCheckState = "all" | "none" | "partial";

  function folderCheckState(folderPath: string): FolderCheckState {
    const under = filePathsUnderFolder(folderPath);
    if (under.length === 0) return "none";
    let selected = 0;
    for (const p of under) {
      if (selectedPaths?.has(p)) selected++;
    }
    if (selected === 0) return "none";
    if (selected === under.length) return "all";
    return "partial";
  }

  function toggleFolderSelection(folderPath: string) {
    const under = filePathsUnderFolder(folderPath);
    if (under.length === 0) return;
    const allSelected = folderCheckState(folderPath) === "all";
    onSelectedPathsChange?.(toggledPaths(selectedPaths, under, allSelected));
  }

  /** Sets the native `indeterminate` property (not an HTML attribute). */
  function indeterminateCheckbox(
    node: HTMLInputElement,
    indeterminate: boolean,
  ): { update: (indeterminate: boolean) => void } {
    node.indeterminate = indeterminate;
    return {
      update(next: boolean) {
        node.indeterminate = next;
      },
    };
  }

  const visibleFilePaths = $derived(
    displayTree
      .filter((n): n is typeof n & { file: FileSnapshot } => n.kind === "file" && !!n.file)
      .map((n) => n.file.path),
  );

  $effect(() => {
    if (!pickerMode) return;
    pickerFocusIdx = 0;
  });

  $effect(() => {
    if (!pickerMode) return;
    const max = visibleFilePaths.length;
    if (max === 0) pickerFocusIdx = 0;
    else if (pickerFocusIdx >= max) pickerFocusIdx = max - 1;
  });

  function onPickerListKeydown(e: KeyboardEvent) {
    if (!pickerMode || visibleFilePaths.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      pickerFocusIdx = (pickerFocusIdx + 1) % visibleFilePaths.length;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      pickerFocusIdx =
        (pickerFocusIdx - 1 + visibleFilePaths.length) % visibleFilePaths.length;
    } else if (e.key === " ") {
      e.preventDefault();
      e.stopPropagation();
      const path = visibleFilePaths[pickerFocusIdx];
      if (path) toggleSelection(path);
    } else if (e.key === "Enter" && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      e.stopPropagation();
      onPickerEnter?.();
    }
  }

  function toggleFolder(folderPath: string) {
    fileTreeCollapse.toggle(folderPath);
  }

  function indentPx(depth: number): string {
    return `${6 + depth * 12}px`;
  }

  const showTreeContent = $derived(
    pickerMode
      ? sourceFiles.length > 0 && displayTree.length > 0
      : !!snapshot && displayTree.length > 0,
  );

  const treeEmptyMessage = $derived.by(() => {
    if (pickerMode) {
      if (sourceFiles.length === 0) return "No files";
      return displayTree.length === 0 ? "No matching files" : null;
    }
    if (!snapshot) return "Loading…";
    return displayTree.length === 0 ? "No files" : null;
  });
</script>

{#if collapsed && !pickerMode}
  <div class="w-10 border-r border-hairline bg-surface flex flex-col items-center py-3 gap-2 transition-[width] duration-200">
    <button type="button"
      onclick={() => app.togglePanel("tree")}
      title="Show file tree"
      aria-label="Show file tree"
      class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center text-fg-3"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
    </button>
    {#if snapshot}
      <div class="mono text-[10px] text-muted [writing-mode:vertical-rl] mt-2">
        {files.length} files · {snapshot.reviewed_count}/{snapshot.total_count}
      </div>
    {/if}
  </div>
{:else}
<div
  class="{embedded
    ? 'flex flex-col flex-1 min-h-0 overflow-hidden bg-surface'
    : 'w-64 border-r border-hairline bg-surface flex flex-col overflow-hidden transition-[width] duration-200'}"
>
  <div class="shrink-0 border-b border-hairline">
    <div class="px-2.5 pt-2.5 pb-1.5">
      <div class="flex items-center gap-1.5 h-7 px-2 rounded-[7px] border border-hairline bg-card text-fg-3 focus-within:border-fg-3">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
        <input
          bind:this={inputEl}
          class="flex-1 bg-transparent text-[12px] text-fg placeholder:text-fg-3 outline-none min-w-0"
          placeholder="Filter files…"
          value={filterDraft}
          oninput={onFilterInput}
          onkeydown={onFilterKeydown}
          onfocus={onFilterFocus}
          onblur={onFilterBlur}
        />
        <span class="kbd">/</span>
      </div>
      <!-- Quick filters render in normal flow ABOVE the results: all of them
           while the query is empty, only matching kinds while typing `kind:`,
           none otherwise — so the live matching files below stay visible rather
           than sitting under a dropdown. -->
      {#if !pickerMode && inputFocused && suggestions.length > 0}
        <div
          class="mt-1.5 grid p-1 rounded-lg border border-hairline bg-card shadow-lg max-h-48 overflow-y-auto"
          bind:this={suggestionsEl}
        >
          {#each suggestions as sug, i (i)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="grid grid-cols-[50px_minmax(0,auto)_minmax(0,1fr)] gap-2 items-center px-2 py-[5px] rounded-[5px] text-[12.5px] cursor-pointer hover:bg-hover
                {i === highlight ? 'bg-hover' : ''}"
              data-sugg-idx={i}
              aria-selected={i === highlight}
              onmousedown={(e) => { e.preventDefault(); pickSuggestion(sug.expr); }}
            >
              <span class="mono text-[10px] tracking-[0.04em] {sug.kind === 'history' ? 'text-muted' : 'text-accent'}">{suggestionLabel(sug)}</span>
              <span class="mono font-medium text-fg truncate min-w-0">{sug.kind === 'preset' ? sug.name : sug.expr}</span>
              <span class="mono text-[11px] text-fg-3 truncate text-right">
                {#if sug.kind === 'kind'}{sug.files ?? 0} {sug.files === 1 ? 'file' : 'files'}{:else if sug.kind === 'preset'}{sug.expr}{/if}
              </span>
            </div>
          {/each}
          {#if snapshot}
            <div class="border-t border-hairline mt-1 pt-1.5 px-2 pb-0.5 text-[11px] text-fg-3">
              {suggestionFooter(files.length, snapshot.reviewed_count, snapshot.total_count)}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  {#if snapshot || pickerMode}
    <div class="flex items-center justify-between px-3 py-1.5 border-b border-hairline text-[11px] mono text-muted shrink-0">
      <span>{files.length} files</span>
      {#if pickerMode && selectedPaths}
        <span>{selectedPaths.size} selected</span>
      {:else if !pickerMode && snapshot}
        <span>{snapshot.reviewed_count}/{snapshot.total_count} reviewed</span>
      {/if}
    </div>
  {/if}

  {#if !pickerMode && snapshot && snapshot.total_count > 0}
    <!-- Review progress meter: single fill (O(1) DOM regardless of file count) -->
    <div class="px-3 pb-1.5 shrink-0">
      <div class="h-[3px] rounded-[2px] overflow-hidden" style="background: color-mix(in srgb, var(--color-fg) 6%, transparent)">
        <div
          class="h-full rounded-[2px] transition-[width] duration-150"
          style="width: {(snapshot.reviewed_count / snapshot.total_count) * 100}%; background: var(--color-periwinkle)"
        ></div>
      </div>
    </div>
  {/if}

  <div
    role="tree"
    aria-label={pickerMode ? "Select files to review" : "Changed files"}
    bind:this={listEl}
    class="flex-1 overflow-y-auto text-[13px] outline-none"
    tabindex={pickerMode ? 0 : undefined}
    onscroll={() => (scrollTop = listEl?.scrollTop ?? 0)}
    onkeydown={pickerMode ? onPickerListKeydown : undefined}
  >
    {#if showTreeContent}
      <div style="padding-top: {vw.paddingTop}px; padding-bottom: {vw.paddingBottom}px;">
        {#each visibleNodes as node (node.fullPath)}
          {#if node.kind === "folder"}
            {@const folderCollapsed = fileTreeCollapse.isCollapsed(node.fullPath)}
            {@const folderState = pickerMode ? folderCheckState(node.fullPath) : null}
            <div
              role="treeitem"
              aria-level={node.depth + 1}
              aria-expanded={!folderCollapsed}
              aria-selected={false}
              tabindex="-1"
              class="flex items-center gap-1.5 pr-2 text-[11px] text-fg-3 cursor-pointer transition-colors duration-75 hover:bg-hover"
              style="padding-top: 4px; padding-bottom: 4px; padding-left: {indentPx(node.depth)};"
              onclick={(e) => {
                if (pickerMode && (e.target as HTMLElement).closest("input[type=checkbox]")) return;
                toggleFolder(node.fullPath);
              }}
              onkeydown={(e) => e.key === "Enter" && toggleFolder(node.fullPath)}
            >
              {#if pickerMode && folderState}
                <input
                  type="checkbox"
                  class="shrink-0 size-3.5 accent-accent"
                  checked={folderState === "all"}
                  use:indeterminateCheckbox={folderState === "partial"}
                  onclick={(e) => e.stopPropagation()}
                  onchange={() => toggleFolderSelection(node.fullPath)}
                  aria-label="Select all files in {node.name}"
                />
              {/if}
              <svg
                width="9"
                height="9"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
                class="shrink-0 transition-transform {folderCollapsed ? '' : 'rotate-90'}"
              >
                <polyline points="9 18 15 12 9 6" />
              </svg>
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="shrink-0 opacity-80"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
              <span class="truncate">{node.name}</span>
            </div>
          {:else if node.file}
            {@const file = resolveTreeFile(filesByPath, node) ?? node.file}
            {@const selected = !pickerMode && selectedFile?.path === file.path}
            {@const inViewport = !pickerMode && !selected && viewportPath === file.path}
            {@const pickerFocused = pickerMode && visibleFilePaths[pickerFocusIdx] === file.path}
            {@const checked = pickerMode && isPathSelected(file.path)}
            <FileRailRow
              {file}
              depth={node.depth}
              {selected}
              {inViewport}
              {pickerMode}
              {pickerFocused}
              {checked}
              annotationTitle={annotationTitle(file)}
              onActivate={() =>
                pickerMode ? toggleSelection(file.path) : jumpToFile(file.path, file.source_index)}
              onTogglePick={() => toggleSelection(file.path)}
            />
          {/if}
        {/each}
      </div>
    {:else if treeEmptyMessage}
      <div class="flex items-center justify-center h-full text-muted text-sm">{treeEmptyMessage}</div>
    {/if}
  </div>

  {#if snapshot && !pickerMode}
    <ScopeSelector
      mode={snapshot.mode}
      total_count={snapshot.total_count}
      reviewed_count={snapshot.reviewed_count}
    />
  {/if}
</div>
{/if}
