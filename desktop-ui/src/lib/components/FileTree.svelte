<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { buildTree, filesByPathMap, resolveTreeFile, visibleTree } from "$lib/treeFromPaths";
  import ScopeSelector from "./ScopeSelector.svelte";
  import { onDestroy } from "svelte";
  import { windowFromScroll } from "$lib/virtualWindow";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { diffScroll } from "$lib/stores/diffScroll.svelte";
  import { fileTreeCollapse } from "$lib/stores/fileTreeCollapse.svelte";
  import type { FileSnapshot } from "$lib/types";

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
    pickerMode ? null : snapshot ? sourceFiles[snapshot.selected_file] : null,
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

  function onFilterInput(e: Event) {
    filterDraft = (e.target as HTMLInputElement).value;
    if (pickerMode) return;
    clearTimer();
    const v = filterDraft;
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      applyFilter(v);
    }, DEBOUNCE_MS);
  }

  function onFilterKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      if (pickerMode) return;
      applyFilter(filterDraft);
      inputEl?.blur();
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
    filterDraft = expr;
    applyFilter(expr);
  }

  onDestroy(clearTimer);

  // ── Virtualization ────────────────────────────────────────────────────────
  const VIRTUALIZE_THRESHOLD = 1000;
  const ROW_HEIGHT = 28;
  let listEl: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let viewportHeight = $state(0);

  $effect(() => {
    if (!listEl) return;
    const ro = new ResizeObserver(() => {
      viewportHeight = listEl!.clientHeight;
    });
    ro.observe(listEl);
    viewportHeight = listEl.clientHeight;
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
    const cur = selectedPaths ?? new Set<string>();
    const next = new Set(cur);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    onSelectedPathsChange?.(next);
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
    const next = new Set(selectedPaths ?? []);
    const state = folderCheckState(folderPath);
    for (const p of under) {
      if (state === "all") next.delete(p);
      else next.add(p);
    }
    onSelectedPathsChange?.(next);
  }

  /** Sets the native `indeterminate` property (not an HTML attribute). */
  function indeterminateCheckbox(
    node: HTMLInputElement,
    indeterminate: boolean,
  ): { update: (indeterminate: boolean) => void } {
    node.indeterminate = indeterminate;
    return {
      update(indeterminate: boolean) {
        node.indeterminate = indeterminate;
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

  function filenameClass(file: { reviewed: boolean; status: string }, selected: boolean): string {
    if (file.status === "deleted") return "text-muted line-through";
    if (file.reviewed) return "text-muted line-through";
    if (selected) return "font-semibold text-fg";
    return "text-fg-2";
  }

  // ── File-type chip helpers ────────────────────────────────────────────────
  function fileExtension(path: string): string {
    const name = path.split("/").pop() ?? path;
    // Handle compound extensions like .test.ts → take last segment
    const parts = name.split(".");
    if (parts.length < 2) return "";
    return parts[parts.length - 1].toLowerCase();
  }

  interface ExtChip { label: string; color: string }

  function extChip(ext: string): ExtChip {
    switch (ext) {
      case "ts":
      case "tsx":   return { label: ext === "tsx" ? "TSX" : "TS",  color: "#5db4ff" };
      case "js":
      case "jsx":   return { label: ext === "jsx" ? "JSX" : "JS",  color: "#ffce5b" };
      case "svelte": return { label: "SV",  color: "#ff7a2b" };
      case "css":
      case "scss":  return { label: ext.toUpperCase().slice(0, 3), color: "#7f87ff" };
      case "rs":    return { label: "RS",   color: "#f46623" };
      case "md":    return { label: "MD",   color: "#aab0bd" };
      case "json":  return { label: "JSON", color: "#aab0bd" };
      case "toml":  return { label: "TOML", color: "#aab0bd" };
      case "yaml":
      case "yml":   return { label: "YML",  color: "#aab0bd" };
      case "html":  return { label: "HTML", color: "#e67e22" };
      case "py":    return { label: "PY",   color: "#4caf50" };
      case "go":    return { label: "GO",   color: "#00acd7" };
      case "sh":
      case "bash":  return { label: "SH",   color: "#aab0bd" };
      default:      return { label: ext ? ext.toUpperCase().slice(0, 3) : "·", color: "#6b6b6b" };
    }
  }

  const showTreeContent = $derived(
    pickerMode
      ? sourceFiles.length > 0 && displayTree.length > 0
      : !!snapshot && displayTree.length > 0,
  );

  const treeEmptyMessage = $derived(
    pickerMode
      ? sourceFiles.length === 0
        ? "No files"
        : displayTree.length === 0
          ? "No matching files"
          : null
      : !snapshot
        ? "Loading…"
        : displayTree.length === 0
          ? "No files"
          : null,
  );
</script>

{#if collapsed && !pickerMode}
  <div class="w-10 border-r border-hairline bg-surface flex flex-col items-center py-3 gap-2 transition-[width] duration-200">
    <button
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
  <div class="relative shrink-0 border-b border-hairline">
    <div class="flex items-center gap-2 px-3 py-2">
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2" class="shrink-0"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
      <input
        bind:this={inputEl}
        class="flex-1 bg-transparent text-sm text-fg-2 placeholder:text-muted outline-none min-w-0"
        placeholder="Filter files…"
        value={filterDraft}
        oninput={onFilterInput}
        onkeydown={onFilterKeydown}
        onfocus={() => (inputFocused = true)}
        onblur={() => setTimeout(() => (inputFocused = false), 150)}
      />
      <span class="kbd">/</span>
    </div>
    {#if !pickerMode && inputFocused && (snapshot?.filter_suggestions?.length ?? 0) > 0}
      <div class="absolute left-0 right-0 top-full z-10 bg-surface border border-hairline border-t-0 max-h-64 overflow-y-auto shadow-lg">
        {#each snapshot?.filter_suggestions ?? [] as sug}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="flex items-center gap-2 px-3 py-1.5 text-[12px] cursor-pointer hover:bg-hover"
            onmousedown={(e) => { e.preventDefault(); pickSuggestion(sug.expr); }}
          >
            <span class="text-[10px] mono uppercase shrink-0 {sug.kind === 'preset' ? 'text-accent' : 'text-muted'}">{sug.kind === 'preset' ? 'preset' : 'recent'}</span>
            {#if sug.kind === 'preset'}
              <span class="text-fg-2 shrink-0">{sug.name}</span>
              <span class="text-muted mono truncate text-[11px]">{sug.expr}</span>
            {:else}
              <span class="text-fg-2 mono truncate">{sug.expr}</span>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
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
    <!-- Review progress meter: segmented bar -->
    <div class="px-3 pb-1.5 shrink-0">
      <div class="flex gap-[2px]">
        {#each { length: snapshot.total_count } as _, i}
          <span
            class="flex-1 h-[3px] rounded-[2px] transition-colors duration-150"
            style="background: {i < snapshot.reviewed_count ? 'var(--color-periwinkle)' : 'rgba(255,255,255,0.06)'}"
          ></span>
        {/each}
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
              class="flex items-center gap-1.5 pr-2 text-[11px] text-fg-3 cursor-pointer hover:bg-card"
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
            {@const file = resolveTreeFile(filesByPath, node)!}
            {@const selected = !pickerMode && selectedFile?.path === file.path}
            {@const inViewport = !pickerMode && !selected && viewportPath === file.path}
            {@const pickerFocused = pickerMode && visibleFilePaths[pickerFocusIdx] === file.path}
            {@const checked = pickerMode && isPathSelected(file.path)}
            {@const chip = extChip(fileExtension(file.path))}
            <div
              role="treeitem"
              aria-level={node.depth + 1}
              aria-selected={pickerMode ? checked : selected}
              tabindex={selected ? 0 : -1}
              class="flex items-center gap-1.5 pr-2 cursor-pointer border-l-2 {pickerFocused ? 'bg-ink-700 border-accent/60' : selected ? 'bg-tree-selected border-accent' : inViewport ? 'border-accent/40 bg-card/60' : checked ? 'bg-card/40 border-transparent' : 'border-transparent hover:bg-card'}"
              style="padding-top: 4px; padding-bottom: 4px; padding-left: {indentPx(node.depth)};"
              onclick={() =>
                pickerMode
                  ? toggleSelection(file.path)
                  : jumpToFile(file.path, file.source_index)}
              onkeydown={(e) => {
                if (e.key !== "Enter") return;
                if (pickerMode) toggleSelection(file.path);
                else jumpToFile(file.path, file.source_index);
              }}
            >
              {#if pickerMode}
                <input
                  type="checkbox"
                  class="shrink-0 size-3.5 accent-accent"
                  checked={checked}
                  onclick={(e) => e.stopPropagation()}
                  onchange={() => toggleSelection(file.path)}
                  aria-label="Include {node.name} in review"
                />
              {/if}
              <!-- File-type chip -->
              <span
                class="shrink-0 inline-flex items-center justify-center w-[14px] h-[14px] rounded-[3px] text-[7px] font-bold leading-none"
                style="background: {chip.color}22; color: {chip.color}"
                aria-hidden="true"
              >{chip.label}</span>

              <span class="truncate flex-1 min-w-0 text-[12px] {filenameClass(file, selected)}">{node.name}</span>

              <!-- Comment/question badge -->
              {#if !file.reviewed && (file.comment_count > 0 || file.question_count > 0)}
                <span
                  title="{(file.comment_count + file.question_count)} comment{(file.comment_count + file.question_count) !== 1 ? 's' : ''}"
                  class="w-[13px] h-[13px] rounded-full shrink-0 flex items-center justify-center text-[9px] font-bold"
                  style="background: rgba(34,211,238,0.15); color: var(--color-comment); border: 1px solid rgba(34,211,238,0.25);"
                >?</span>
              {/if}

              <!-- +N −N counts -->
              {#if file.is_lazy_stub}
                <span class="text-[10px] mono text-muted shrink-0 opacity-40">···</span>
              {:else}
                <span class="flex items-center gap-1 shrink-0">
                  {#if file.additions > 0}
                    <span class="text-[10px] mono text-add-fg">+{file.additions}</span>
                  {/if}
                  {#if file.deletions > 0}
                    <span class="text-[10px] mono text-del-fg">−{file.deletions}</span>
                  {/if}
                </span>
              {/if}
            </div>
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
