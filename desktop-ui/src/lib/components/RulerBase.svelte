<script lang="ts">
  /**
   * Shared body of the Tour and Diff rulers: one tick per file along the
   * scroll edge, and a popover that lists them on hover or keyboard focus.
   * The two rulers differ only in how files are grouped and what the popover
   * heading says, so those arrive as props and the geometry lives in
   * `rulerGeometry`. Prop-driven so the host owns navigation.
   *
   * The root is a reserved strip the host lays out beside the code column,
   * never an overlay on it: an overlay painted its ticks over line numbers
   * and code, and stacked under the sticky file header (z-30) so the header
   * covered both ticks and popover. As a flex sibling the code column simply
   * gets narrower, and the band width the row model measures follows.
   */
  import { extChip } from "$lib/fileTreeModel";
  import { baseName, shortFileName } from "$lib/shortFileName";
  import { rulerFileCount, tickGap, tickState, type RulerGroup, type RulerItem } from "$lib/rulerGeometry";

  interface Props {
    groups: RulerGroup[];
    heading: string;
    label: string;
    onGoto: (path: string) => void;
  }
  const { groups, heading, label, onGoto }: Props = $props();

  let height = $state(0);

  const total = $derived(rulerFileCount(groups));
  // The ruler is vertically centred and may use most of the strip, so the
  // strip's height is the budget a crowded diff has to fit in.
  const gap = $derived(tickGap(total, groups.length, height * 0.8));

  function dir(path: string): string {
    const i = path.lastIndexOf("/");
    return i < 0 ? "" : path.slice(0, i);
  }

  function chipOf(path: string) {
    const name = baseName(path);
    return extChip(name.includes(".") ? (name.split(".").pop() ?? "").toLowerCase() : "");
  }

  // The popover opens on hover or keyboard focus only. A click focuses the
  // ruler too (it has a tabindex), and if that opened it the 340px list would
  // stay pinned over the diff after the pointer left; so going to a file and
  // Escape both drop focus to close it.
  function goto(e: MouseEvent, path: string) {
    onGoto(path);
    (e.currentTarget as HTMLElement).blur();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    const el = document.activeElement;
    if (el instanceof HTMLElement && (e.currentTarget as HTMLElement).contains(el)) el.blur();
  }

  function rowTitle(it: RulerItem): string {
    return it.annotations > 0 ? `${it.path} · ${it.annotations} annotation${it.annotations === 1 ? "" : "s"}` : it.path;
  }
</script>

{#if total > 0}
  <div class="ruler-gutter" bind:clientHeight={height}>
    <div class="ruler" role="toolbar" aria-label={label} tabindex="0" onkeydown={onKeydown}>
      <div class="rticks" style="gap:{gap}px">
        {#each groups as g, gi (gi)}
          {#if gi > 0}<i class="rgap"></i>{/if}
          {#each g.items as it (it.path)}
            <i class="rt {tickState(it)}"></i>
          {/each}
        {/each}
      </div>
      <div class="rpop" role="menu">
        <div class="rpop-h">
          {heading}<span class="count">{total}</span>
        </div>
        <div class="rpop-l">
          {#each groups as g, gi (gi)}
            {#if g.title}<div class="rp-g">{g.title}</div>{/if}
            {#each g.items as it (it.path)}
              {@const short = shortFileName(baseName(it.path))}
              {@const chip = chipOf(it.path)}
              <button
                type="button"
                role="menuitem"
                class="rp-file {it.current ? 'cur' : ''} {it.reviewed ? 'rev' : ''} {g.title ? 'ind' : ''}"
                title={rowTitle(it)}
                onclick={(e) => goto(e, it.path)}
              >
                <span
                  class="chip"
                  style="background: color-mix(in srgb, {chip.color} 13%, transparent); color: {chip.color}"
                  aria-hidden="true">{chip.label}</span
                >
                <span class="nm">{short.main}</span>
                {#if !g.title}<span class="dir">{dir(it.path)}</span>{/if}
                {#if it.annotations > 0}<span class="rp-n">{it.annotations}</span>{/if}
              </button>
            {/each}
          {/each}
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Wide enough for a hovered current tick (14px) plus the ruler's inset.
   * The strip, not the ruler, is the positioned box: the popover centres on
   * the strip and caps its height to it, so a short column cannot push the
   * list past its top or bottom. z-index 35 clears the sticky file header
   * (z-30) and stays under app menus and their backdrops (z-40+). */
  .ruler-gutter {
    position: relative;
    z-index: 35;
    flex: none;
    width: 20px;
    display: flex;
    align-items: center;
  }
  .ruler {
    padding: 6px 2px 6px 4px;
    outline: none;
  }
  .rticks {
    display: grid;
    justify-items: start;
  }
  .rt {
    display: block;
    width: 8px;
    height: 2px;
    border-radius: 1px;
    background: var(--color-fg-3);
    opacity: 0.55;
    transition: width 0.12s;
  }
  .rt.rev {
    opacity: 0.25;
  }
  .rt.has {
    background: var(--color-accent);
    opacity: 0.8;
  }
  .rt.cur {
    width: 14px;
    background: var(--color-fg);
    opacity: 1;
  }
  .rgap {
    display: block;
    height: 6px;
  }
  .ruler:hover .rt,
  .ruler:focus-visible .rt,
  .ruler:has(:focus-visible) .rt {
    width: 12px;
  }
  .ruler:hover .rt.cur,
  .ruler:focus-visible .rt.cur,
  .ruler:has(:focus-visible) .rt.cur {
    width: 14px;
  }
  .rpop {
    position: absolute;
    left: 4px;
    top: 50%;
    transform: translateY(-50%);
    width: 340px;
    max-height: min(70vh, 640px, calc(100% - 16px));
    display: none;
    flex-direction: column;
    background: var(--color-card);
    border: 1px solid var(--color-ink-500);
    border-radius: 10px;
    box-shadow: 0 18px 44px rgba(0, 0, 0, 0.35);
    overflow: hidden;
  }
  .ruler:hover .rpop,
  .ruler:focus-visible .rpop,
  .ruler:has(:focus-visible) .rpop {
    display: flex;
  }
  @media (prefers-reduced-motion: no-preference) {
    .ruler:hover .rpop {
      animation: ruler-in 0.14s ease-out;
    }
  }
  @keyframes ruler-in {
    from {
      opacity: 0;
      transform: translateY(-48%);
    }
  }
  .rpop-h {
    display: flex;
    justify-content: space-between;
    padding: 10px 12px 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--color-fg-2);
    border-bottom: 1px solid var(--color-hairline);
  }
  .count {
    font-family: var(--font-mono, monospace);
    font-weight: 400;
    color: var(--color-muted);
  }
  .rpop-l {
    overflow: auto;
    padding: 4px;
    display: grid;
    gap: 1px;
  }
  .rp-g {
    font-size: 12px;
    font-weight: 600;
    padding: 8px 8px 3px;
    color: var(--color-fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rp-file {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 6px;
    text-align: left;
    font-size: 12.5px;
    min-width: 0;
    color: var(--color-fg-2);
  }
  .rp-file.ind {
    padding-left: 20px;
  }
  .rp-file:hover {
    background: var(--color-hover);
  }
  .rp-file.cur {
    background: color-mix(in srgb, var(--color-accent) 12%, var(--color-hover));
  }
  .rp-file.rev .nm {
    color: var(--color-success);
  }
  .chip {
    flex: none;
    width: 26px;
    text-align: center;
    border-radius: 3px;
    font-size: 8.5px;
    font-weight: 700;
    line-height: 14px;
  }
  .nm {
    flex: 0 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dir {
    font-family: var(--font-mono, monospace);
    font-size: 10.5px;
    color: var(--color-muted);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rp-n {
    margin-left: auto;
    font-family: var(--font-mono, monospace);
    font-size: 10.5px;
    color: var(--color-accent);
    flex: none;
  }
  /* Narrow windows stack the Tour rail above the column and have no width to
   * spare for the strip. */
  @media (max-width: 880px) {
    .ruler-gutter {
      display: none;
    }
  }
</style>
