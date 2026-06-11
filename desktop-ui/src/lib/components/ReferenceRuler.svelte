<script lang="ts">
  /**
   * Overview ruler for reference-highlight matches (issue #69) — a thin
   * absolutely-positioned strip on the right edge of the diff scroll
   * viewport, like the scrollbar markers in VSCode/Zed. One mark per matched
   * row (overlapping marks are pre-merged by `buildRulerMarks`); clicking a
   * mark jumps to that match.
   *
   * Hovering a mark for ~150ms shows a context preview popover beside it:
   * the matched line plus up to two adjacent lines from the same hunk, the
   * match emphasized, with a file-path + line-number header. Merged cluster
   * marks preview their first match and note "×N matches". The popover is
   * hover-only and ignores pointer events, so it can never trap the mouse.
   *
   * Rendered only while a highlight is active, so it appears/clears with the
   * highlight itself.
   */
  import { findMatchRanges, type MatchOptions } from "$lib/referenceHighlight";
  import type { RulerMark, UsageContextLine, UsageLine } from "$lib/referenceUsages";

  interface MarkPreview {
    usage: UsageLine;
    lines: UsageContextLine[];
  }

  interface Props {
    marks: RulerMark[];
    onJump: (rowIdx: number) => void;
    /** Context lookup for a mark's (first) matched row. */
    getPreview: (rowIdx: number) => MarkPreview | null;
    /** Active highlight query — used to emphasize matches in preview lines. */
    query: string;
    matchOpts: MatchOptions;
  }
  const { marks, onJump, getPreview, query, matchOpts }: Props = $props();

  const HOVER_DELAY_MS = 150;

  let rulerHeight = $state(0);
  let popH = $state(0);
  let hovered = $state<{ mark: RulerMark; preview: MarkPreview } | null>(null);
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;

  function clearHoverTimer(): void {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }
  }

  function onMarkEnter(mark: RulerMark): void {
    clearHoverTimer();
    hoverTimer = setTimeout(() => {
      hoverTimer = null;
      const preview = getPreview(mark.rowIdx);
      hovered = preview ? { mark, preview } : null;
    }, HOVER_DELAY_MS);
  }

  function onMarkLeave(): void {
    clearHoverTimer();
    hovered = null;
  }

  // Clear a stale preview if the marks list changes underneath it
  // (diff refresh, query edit).
  $effect(() => {
    void marks;
    clearHoverTimer();
    hovered = null;
  });

  /** Keep the preview box inside the ruler's vertical extent. */
  const previewTop = $derived.by(() => {
    if (!hovered) return 0;
    const ideal = hovered.mark.topPx - 14;
    const maxTop = Math.max(4, rulerHeight - (popH || 120) - 4);
    return Math.max(4, Math.min(ideal, maxTop));
  });

  function segments(text: string): Array<{ text: string; match: boolean }> {
    const ranges = findMatchRanges(text, query, matchOpts);
    if (ranges.length === 0) return [{ text, match: false }];
    const out: Array<{ text: string; match: boolean }> = [];
    let pos = 0;
    for (const [s, e] of ranges) {
      if (s > pos) out.push({ text: text.slice(pos, s), match: false });
      out.push({ text: text.slice(s, e), match: true });
      pos = e;
    }
    if (pos < text.length) out.push({ text: text.slice(pos), match: false });
    return out;
  }
</script>

<div bind:clientHeight={rulerHeight} class="ref-ruler" aria-label="Reference match positions">
  {#each marks as m (m.rowIdx)}
    <button
      type="button"
      class="ref-ruler-mark"
      style="top:{m.topPx}px"
      tabindex="-1"
      aria-label="Jump to reference match"
      onclick={() => onJump(m.rowIdx)}
      onmouseenter={() => onMarkEnter(m)}
      onmouseleave={onMarkLeave}
    ></button>
  {/each}

  {#if hovered}
    {@const u = hovered.preview.usage}
    <div
      bind:clientHeight={popH}
      class="pointer-events-none absolute right-3 z-50 w-[380px] max-w-[60vw] bg-card border border-hairline rounded-md shadow-2xl overflow-hidden"
      style="top:{previewTop}px"
      role="tooltip"
    >
      <div class="flex items-baseline gap-2 px-2.5 py-1 border-b border-hairline bg-ink-870 min-w-0">
        <span class="mono text-[11px] text-fg-2 truncate" title={u.filePath}>{u.filePath}</span>
        <span class="mono text-[11px] text-muted whitespace-nowrap ml-auto tabular-nums">
          :{u.lineNum ?? "·"}{hovered.mark.count > 1 ? ` · ×${hovered.mark.count} matches` : ""}
        </span>
      </div>
      <div class="py-1">
        {#each hovered.preview.lines as cl, ci (ci)}
          <div class="flex items-baseline gap-2 px-2.5 mono text-[11px] leading-[1.6] {cl.isMatch ? 'bg-hover' : ''}">
            <span class="w-9 shrink-0 text-right text-muted tabular-nums">{cl.lineNum ?? "·"}</span>
            <span class="whitespace-pre overflow-hidden text-ellipsis {cl.isMatch ? 'text-fg' : 'text-fg-3'}">
              {#each segments(cl.text) as seg, si (si)}
                {#if seg.match}<span class="text-periwinkle font-medium">{seg.text}</span>{:else}{seg.text}{/if}
              {/each}
            </span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>
