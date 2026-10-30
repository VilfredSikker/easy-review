<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { FileSnapshot } from "$lib/types";
  import { extChip } from "$lib/fileTreeModel";
  import { riskDotClass } from "$lib/fileStatus";
  import { baseName, shortFileName } from "$lib/shortFileName";

  interface Props {
    file: FileSnapshot;
    depth: number;
    /** Selected in the backend (the file the diff cursor is on). */
    selected: boolean;
    /** Scrolled into view in the diff but not the selected file. */
    inViewport: boolean;
    /** AI file picker: the row is a multi-select checkbox, not a reviewed toggle. */
    pickerMode?: boolean;
    pickerFocused?: boolean;
    checked?: boolean;
    annotationTitle: string;
    onActivate: () => void;
    onTogglePick?: () => void;
  }
  const {
    file,
    depth,
    selected,
    inViewport,
    pickerMode = false,
    pickerFocused = false,
    checked = false,
    annotationTitle,
    onActivate,
    onTogglePick,
  }: Props = $props();

  const name = $derived(baseName(file.path));
  const short = $derived(shortFileName(name));
  const chip = $derived(extChip(name.includes(".") ? (name.split(".").pop() ?? "").toLowerCase() : ""));
  const isRename = $derived(file.status === "renamed" || !!file.old_path);
  const hasAnnotations = $derived(
    file.comment_count > 0 || file.question_count > 0 || file.finding_count > 0,
  );

  function rowClass(): string {
    if (pickerFocused) return "bg-ink-700 border-accent/60";
    if (selected) return "bg-tree-selected border-accent";
    if (inViewport) return "border-accent/40 bg-card/60 hover:bg-hover";
    if (checked) return "bg-card/40 border-transparent hover:bg-hover";
    return "border-transparent hover:bg-hover";
  }

  function nameClass(): string {
    if (file.status === "deleted" || file.reviewed) return "text-muted line-through";
    if (selected) return "font-semibold text-fg";
    return "text-fg-2";
  }

  /** The checkbox is a sibling target of the row click, so it must not also
   *  jump to the file. The write paints at once and rolls back on failure
   *  (ADR 0017), so the box answers the click without waiting for the engine. */
  function toggleReviewed(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    void app.cmd(file.reviewed ? "unmark_reviewed" : "mark_reviewed", { path: file.path });
  }
</script>

<div
  role="treeitem"
  aria-level={depth + 1}
  aria-selected={pickerMode ? checked : selected}
  tabindex={selected ? 0 : -1}
  class="flex items-center gap-1.5 pr-2 cursor-pointer border-l-2 transition-colors duration-75 {rowClass()}"
  style="padding-top: 4px; padding-bottom: 4px; padding-left: {6 + depth * 12}px;"
  title={file.old_path ? `${file.old_path} → ${file.path}` : file.path}
  onclick={onActivate}
  onkeydown={(e) => e.key === "Enter" && onActivate()}
>
  {#if pickerMode}
    <input
      type="checkbox"
      class="shrink-0 size-3.5 accent-accent"
      {checked}
      onclick={(e) => e.stopPropagation()}
      onchange={() => onTogglePick?.()}
      aria-label="Include {name} in review"
    />
  {:else}
    <button
      type="button"
      role="checkbox"
      aria-checked={file.reviewed}
      aria-label={file.reviewed ? "Unmark as reviewed" : "Mark as reviewed"}
      title={file.reviewed ? "Reviewed — click to unmark" : "Mark reviewed"}
      class="shrink-0 w-[14px] h-[14px] rounded-[4px] border-[1.5px] flex items-center justify-center transition-colors
        {file.reviewed
        ? 'bg-add-fg border-add-fg text-ground'
        : 'bg-card border-muted text-transparent hover:border-fg-3'}"
      onclick={toggleReviewed}
    >
      <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.5" aria-hidden="true">
        <polyline points="20 6 9 17 4 12" />
      </svg>
    </button>
  {/if}

  <span
    class="shrink-0 inline-flex items-center justify-center w-[14px] h-[14px] rounded-[3px] text-[7px] font-bold leading-none"
    style="background: color-mix(in srgb, {chip.color} 13%, transparent); color: {chip.color}"
    aria-hidden="true"
  >{chip.label}</span>

  <!-- A dot per row for the review's file-level risk; a file the review never
       reached carries none, which is the distinction the dot makes. -->
  {#if file.risk}
    <span class="w-1.5 h-1.5 rounded-full shrink-0 {riskDotClass(file.risk)}" title="Risk: {file.risk}" aria-hidden="true"></span>
  {/if}

  <span class="flex items-center gap-1.5 flex-1 min-w-0 text-[12px]">
    <span class="truncate {nameClass()}">{short.main}</span>
    {#if short.stamp}
      <span
        class="shrink-0 mono text-[10px] text-muted px-1 rounded-[3px] bg-card leading-[15px]"
        title="version {short.version}"
      >{short.stamp}</span>
    {/if}
    {#if file.kind === "test"}
      <span class="shrink-0 mono text-[8px] font-semibold tracking-wide px-1 rounded-[3px] bg-card text-fg-3">TEST</span>
    {/if}
  </span>

  {#if hasAnnotations}
    <span
      title={annotationTitle}
      class="w-[13px] h-[13px] rounded-full shrink-0 flex items-center justify-center text-[9px] font-bold"
      style="background: color-mix(in srgb, var(--color-comment) 15%, transparent); color: var(--color-comment); border: 1px solid color-mix(in srgb, var(--color-comment) 25%, transparent);"
    >?</span>
  {/if}

  {#if isRename}
    <span class="text-[10px] mono text-muted shrink-0" title={file.similarity != null ? `Renamed, ${file.similarity}% similar` : "Renamed"}>R</span>
  {/if}
  {#if file.is_lazy_stub}
    <span class="text-[10px] mono text-muted shrink-0 opacity-40">···</span>
  {:else}
    <span class="flex items-center gap-1 shrink-0">
      {#if file.additions > 0}<span class="text-[10px] mono text-add-fg">+{file.additions}</span>{/if}
      {#if file.deletions > 0}<span class="text-[10px] mono text-del-fg">−{file.deletions}</span>{/if}
    </span>
  {/if}
</div>
