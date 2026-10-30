<script lang="ts">
  import { tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { findingsVisibility } from "$lib/stores/findingsVisibility.svelte";
  import { displayMenuCounts } from "$lib/viewBarModel";
  import type { CommentVisibility } from "$lib/commentVisibility";

  /**
   * The diff display menu: layout, wrap, line numbers and which annotation
   * kinds the diff draws. Each row shows the total of its kind whatever the
   * current filters, so it says what turning the row on would reveal. Rows
   * apply at once and the menu stays open, so several can be flipped in a row.
   */
  let open = $state(false);
  let root = $state<HTMLDivElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);
  let menu = $state<HTMLDivElement | null>(null);

  const counts = $derived(displayMenuCounts(app.snapshot));
  const vis = $derived(app.commentVisibility);

  // `s` is the global split toggle; the hint teaches it where it applies.
  const LAYOUT_ROWS = [
    { mode: "unified", label: "Unified", hint: "" },
    { mode: "split", label: "Split", hint: "s" },
  ] as const;

  type HideKey = keyof Pick<CommentVisibility, "hideComments" | "hideFindings" | "hideQuestions" | "hideNotes">;
  const annotationRows = $derived<{ key: HideKey; label: string; count: number }[]>([
    { key: "hideComments", label: "Comments", count: counts.comments },
    { key: "hideFindings", label: "Findings", count: counts.findingsUnresolved },
    { key: "hideQuestions", label: "Questions", count: counts.questions },
    { key: "hideNotes", label: "Notes", count: counts.notes },
  ]);

  function items(): HTMLElement[] {
    return Array.from(menu?.querySelectorAll<HTMLElement>("[role^='menuitem']") ?? []);
  }

  async function show() {
    open = true;
    await tick();
    items()[0]?.focus();
  }

  function close(refocus: boolean) {
    open = false;
    if (refocus) trigger?.focus();
  }

  function toggleHide(key: HideKey) {
    app.setCommentVisibility({ [key]: !vis[key] });
  }

  function openSettings() {
    close(false);
    app.setMainView("settings");
  }

  function onMenuKey(e: KeyboardEvent) {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Home" && e.key !== "End") return;
    e.preventDefault();
    const list = items();
    if (list.length === 0) return;
    // Nothing focused yet reads as "before the first row", so ↑ wraps to the last.
    const focused = list.indexOf(document.activeElement as HTMLElement);
    const at = focused < 0 && e.key === "ArrowUp" ? 0 : focused;
    const step: Record<string, number> = {
      Home: -at,
      End: list.length - 1 - at,
      ArrowDown: 1,
      ArrowUp: -1,
    };
    list[(at + step[e.key] + list.length) % list.length]?.focus();
  }

  // Capture phase, so Escape closes this menu and no global Escape binding
  // acts on the same keypress.
  function onWindowKey(e: KeyboardEvent) {
    if (!open || e.key !== "Escape") return;
    e.preventDefault();
    e.stopImmediatePropagation();
    close(true);
  }

  function onWindowPointer(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) close(false);
  }

  $effect(() => {
    window.addEventListener("keydown", onWindowKey, true);
    window.addEventListener("pointerdown", onWindowPointer, true);
    return () => {
      window.removeEventListener("keydown", onWindowKey, true);
      window.removeEventListener("pointerdown", onWindowPointer, true);
    };
  });

  const rowClass =
    "grid w-full grid-cols-[16px_minmax(0,1fr)_auto] items-center gap-1.5 rounded-md px-2 py-1.5 text-left text-[13px] text-fg hover:bg-card focus-visible:bg-card focus-visible:outline-none";
</script>

<div class="relative" bind:this={root}>
  <button
    bind:this={trigger}
    type="button"
    class="grid h-7 w-7 shrink-0 place-items-center rounded-[7px] hover:bg-card hover:text-fg {open
      ? 'bg-card text-accent'
      : 'text-fg-3'}"
    aria-label="Diff display settings"
    aria-haspopup="menu"
    aria-expanded={open}
    title="Diff display: layout and annotations"
    data-testid="display-menu-button"
    onclick={() => (open ? close(false) : void show())}
  >
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
      <path d="M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6" /><circle cx="10" cy="4.5" r="1.5" /><circle cx="6" cy="11.5" r="1.5" />
    </svg>
  </button>

  {#if open}
    <div
      bind:this={menu}
      class="absolute right-0 top-full z-[35] mt-1.5 grid w-[230px] rounded-[10px] border border-border bg-surface p-1.5 shadow-[0_16px_40px_rgba(0,0,0,0.3)]"
      role="menu"
      tabindex="-1"
      aria-label="Diff display"
      data-testid="display-menu"
      onkeydown={onMenuKey}
    >
      <div class="px-2 pb-1 pt-1.5 text-[10.5px] font-semibold uppercase tracking-[0.06em] text-muted">Layout</div>
      {#each LAYOUT_ROWS as row (row.mode)}
        <button
          type="button"
          class={rowClass}
          role="menuitemradio"
          aria-checked={app.diffViewMode === row.mode}
          onclick={() => app.setDiffViewMode(row.mode)}
        >
          <span class="text-[12px] text-accent" aria-hidden="true">{app.diffViewMode === row.mode ? "✓" : ""}</span>
          {row.label}
          {#if row.hint}<kbd class="font-mono text-[11px] text-muted">{row.hint}</kbd>{/if}
        </button>
      {/each}
      <button type="button" class={rowClass} role="menuitemcheckbox" aria-checked={app.wrapLines} onclick={() => app.toggleWrapLines()}>
        <span class="text-[12px] text-accent" aria-hidden="true">{app.wrapLines ? "✓" : ""}</span>
        Wrap long lines
      </button>
      <button
        type="button"
        class={rowClass}
        role="menuitemcheckbox"
        aria-checked={app.lineNumbers}
        onclick={() => app.setLineNumbers(!app.lineNumbers)}
      >
        <span class="text-[12px] text-accent" aria-hidden="true">{app.lineNumbers ? "✓" : ""}</span>
        Line numbers
      </button>

      <div class="mx-0.5 my-1 h-px bg-line" role="separator"></div>
      <div class="px-2 pb-1 pt-1.5 text-[10.5px] font-semibold uppercase tracking-[0.06em] text-muted">Annotations</div>
      {#each annotationRows as row (row.key)}
        <button
          type="button"
          class={rowClass}
          role="menuitemcheckbox"
          aria-checked={!vis[row.key]}
          onclick={() => toggleHide(row.key)}
        >
          <span class="text-[12px] text-accent" aria-hidden="true">{vis[row.key] ? "" : "✓"}</span>
          {row.label}
          <span class="font-mono text-[11px] text-muted">{row.count}</span>
        </button>
      {/each}
      <button
        type="button"
        class={rowClass}
        role="menuitemcheckbox"
        aria-checked={findingsVisibility.showResolved}
        title="For this session only, like the reviewer's other finding filters"
        onclick={() => findingsVisibility.toggleResolved()}
      >
        <span class="text-[12px] text-accent" aria-hidden="true">{findingsVisibility.showResolved ? "✓" : ""}</span>
        Resolved findings
        <span class="font-mono text-[11px] text-muted">{counts.findingsResolved}</span>
      </button>

      <div class="mx-0.5 my-1 h-px bg-line" role="separator"></div>
      <button type="button" class={rowClass} role="menuitem" onclick={openSettings}>
        <span aria-hidden="true"></span>
        All display settings…
      </button>
    </div>
  {/if}
</div>
