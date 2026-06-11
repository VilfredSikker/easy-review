<script lang="ts">
  /**
   * Cmd+click usages popover (issue #69) — a compact, fixed-position panel
   * anchored near the clicked token, listing every word-boundary usage of
   * the highlighted identifier across the rendered diff, grouped by file.
   *
   * - Click a usage (or ArrowUp/Down + Enter) to jump: scroll to the row and
   *   pulse it with the existing `.flash` animation, then close.
   * - Each row has a chevron that expands ~2 adjacent context lines inline
   *   under the row (match emphasized). Multiple rows can be expanded;
   *   expansion never affects keyboard navigation — arrows still move
   *   between usage rows and Enter still jumps.
   * - Esc closes the popover first (global handler in `keyboard.ts`); a
   *   second Esc clears the highlight. Click-outside also closes.
   * - The list is capped (first 100 usages) with a "+N more" footer.
   */
  import { tick } from "svelte";
  import { refHighlight } from "$lib/stores/referenceHighlight.svelte";
  import { findMatchRanges } from "$lib/referenceHighlight";
  import {
    clampPopoverPosition,
    groupUsagesByFile,
    usagePreview,
    type UsageContextLine,
    type UsageLine,
  } from "$lib/referenceUsages";

  interface Props {
    identifier: string;
    usages: UsageLine[];
    anchor: { x: number; y: number } | null;
    onJump: (rowIdx: number) => void;
    /** Surrounding lines for a usage (shown when a row is expanded). */
    getContext: (u: UsageLine) => UsageContextLine[];
  }
  const { identifier, usages, anchor, onJump, getContext }: Props = $props();

  const USAGE_CAP = 100;
  const grouped = $derived(groupUsagesByFile(usages, USAGE_CAP));
  /** Capped flat list in display order — index space for keyboard nav. */
  const flat = $derived(grouped.groups.flatMap((g) => g.usages));
  const overflow = $derived(grouped.total - grouped.shown);
  const fileCount = $derived(grouped.groups.length);

  let el = $state<HTMLDivElement | null>(null);
  let popW = $state(0);
  let popH = $state(0);
  let selectedIdx = $state(0);
  /** Flat indices with their context lines expanded inline. */
  let expanded = $state(new Set<number>());

  // Reset keyboard selection and expansion whenever the identifier (and thus
  // the list) changes.
  $effect(() => {
    void identifier;
    selectedIdx = 0;
    expanded = new Set();
  });

  function toggleExpanded(i: number): void {
    const next = new Set(expanded);
    if (next.has(i)) next.delete(i);
    else next.add(i);
    expanded = next;
  }

  const pos = $derived(
    clampPopoverPosition(
      (anchor?.x ?? 0) + 10,
      (anchor?.y ?? 0) + 14,
      popW || 400,
      popH || 320,
      typeof window === "undefined" ? 1280 : window.innerWidth,
      typeof window === "undefined" ? 800 : window.innerHeight,
    ),
  );

  function fileName(path: string): string {
    const i = path.lastIndexOf("/");
    return i === -1 ? path : path.slice(i + 1);
  }
  function fileDir(path: string): string {
    const i = path.lastIndexOf("/");
    return i === -1 ? "" : path.slice(0, i + 1);
  }

  function jump(i: number): void {
    const u = flat[i];
    if (!u) return;
    onJump(u.rowIdx);
    refHighlight.closePopover();
  }

  async function move(delta: number): Promise<void> {
    if (flat.length === 0) return;
    selectedIdx = Math.max(0, Math.min(flat.length - 1, selectedIdx + delta));
    await tick();
    el?.querySelector(`[data-usage-idx="${selectedIdx}"]`)?.scrollIntoView({ block: "nearest" });
  }

  /** Match emphasis for expanded context lines (popover = identifier mode). */
  function segments(text: string): Array<{ text: string; match: boolean }> {
    const ranges = findMatchRanges(text, identifier, refHighlight.matchOptions);
    if (ranges.length === 0) return [{ text, match: false }];
    const out: Array<{ text: string; match: boolean }> = [];
    let posIdx = 0;
    for (const [s, e] of ranges) {
      if (s > posIdx) out.push({ text: text.slice(posIdx, s), match: false });
      out.push({ text: text.slice(s, e), match: true });
      posIdx = e;
    }
    if (posIdx < text.length) out.push({ text: text.slice(posIdx), match: false });
    return out;
  }

  // Keyboard navigation while open. The global capture handler in keyboard.ts
  // owns Esc (popover-close precedence); arrows/Enter are unclaimed there.
  $effect(() => {
    function onKeydown(e: KeyboardEvent) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        e.stopPropagation();
        void move(1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        void move(-1);
      } else if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        jump(selectedIdx);
      }
    }
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  // Click-outside closes. The opening click's mousedown happened before this
  // effect mounts, so the popover never closes itself on open.
  $effect(() => {
    function onDocMouseDown(e: MouseEvent) {
      if (el && e.target instanceof Node && el.contains(e.target)) return;
      refHighlight.closePopover();
    }
    window.addEventListener("mousedown", onDocMouseDown);
    return () => window.removeEventListener("mousedown", onDocMouseDown);
  });
</script>

<div
  bind:this={el}
  bind:clientWidth={popW}
  bind:clientHeight={popH}
  class="fixed z-50 w-[400px] max-w-[calc(100vw-16px)] bg-card border border-border rounded-md shadow-2xl overflow-hidden flex flex-col"
  style="left:{pos.left}px; top:{pos.top}px"
  data-modal
  role="dialog"
  aria-label="Usages of {identifier}"
>
  <div class="flex items-baseline gap-2 px-3 py-1.5 border-b border-hairline bg-ink-870 shrink-0 min-w-0">
    <span class="mono text-xs text-fg truncate">{identifier}</span>
    <span class="text-[11px] text-muted whitespace-nowrap ml-auto">
      {grouped.total} {grouped.total === 1 ? "usage" : "usages"} · {fileCount} {fileCount === 1 ? "file" : "files"}
    </span>
  </div>

  <div class="overflow-y-auto max-h-[320px] py-1">
    {#if flat.length === 0}
      <div class="px-3 py-2 text-xs text-muted">No usages in the rendered diff</div>
    {/if}
    {#each flat as u, i (i)}
      {#if i === 0 || flat[i - 1].filePath !== u.filePath}
        <div class="px-3 pt-1.5 pb-0.5 text-[11px] mono truncate" title={u.filePath}>
          <span class="text-muted">{fileDir(u.filePath)}</span><span class="text-fg-2">{fileName(u.filePath)}</span>
        </div>
      {/if}
      {@const p = usagePreview(u.text, u.ranges[0])}
      {@const isExpanded = expanded.has(i)}
      <div
        class="flex items-stretch hover:bg-hover {i === selectedIdx ? 'bg-hover' : ''}"
        data-usage-idx={i}
        onmousemove={() => (selectedIdx = i)}
        role="presentation"
      >
        <button
          type="button"
          class="pl-2 pr-0.5 shrink-0 flex items-center text-muted hover:text-fg-2"
          onclick={(e) => {
            e.stopPropagation();
            toggleExpanded(i);
          }}
          title={isExpanded ? "Hide context" : "Show context"}
          aria-label={isExpanded ? "Hide context lines" : "Show context lines"}
          aria-expanded={isExpanded}
        >
          <svg
            width="9"
            height="9"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            class="transition-transform {isExpanded ? 'rotate-90' : ''}"
          >
            <polyline points="9 18 15 12 9 6" />
          </svg>
        </button>
        <button
          type="button"
          class="flex-1 min-w-0 flex items-baseline gap-2 pr-3 py-0.5 text-left mono text-xs"
          onclick={() => jump(i)}
        >
          <span class="w-9 shrink-0 text-right text-muted tabular-nums">{u.lineNum ?? "·"}</span>
          <span class="truncate whitespace-pre">
            <span class="text-fg-3">{p.prefix}</span><span class="text-periwinkle font-medium">{p.match}</span><span class="text-fg-3">{p.suffix}</span>
          </span>
          {#if u.ranges.length > 1}
            <span class="ml-auto shrink-0 text-[10px] text-muted">×{u.ranges.length}</span>
          {/if}
        </button>
      </div>
      {#if isExpanded}
        <div class="pl-[22px] pb-1 bg-ink-870/60 border-y border-hairline">
          {#each getContext(u) as cl, ci (ci)}
            <div class="flex items-baseline gap-2 pr-3 mono text-[11px] leading-[1.6]">
              <span class="w-9 shrink-0 text-right text-muted tabular-nums">{cl.lineNum ?? "·"}</span>
              <span class="whitespace-pre overflow-hidden text-ellipsis {cl.isMatch ? 'text-fg' : 'text-fg-3'}">
                {#each segments(cl.text) as seg, si (si)}
                  {#if seg.match}<span class="text-periwinkle font-medium">{seg.text}</span>{:else}{seg.text}{/if}
                {/each}
              </span>
            </div>
          {/each}
        </div>
      {/if}
    {/each}
  </div>

  {#if overflow > 0}
    <div class="px-3 py-1 border-t border-hairline text-[11px] text-muted shrink-0">
      +{overflow} more
    </div>
  {/if}
</div>
