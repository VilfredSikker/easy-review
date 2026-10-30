<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { copyToClipboard } from "$lib/clipboard";
  import {
    EXPORT_CATEGORIES,
    highlightMarkdown,
    isPickable,
    itemLocation,
    previewSize,
    visibleItems,
    type ExportCategory,
    type ExportItem,
  } from "$lib/exportOptions";
  import { app } from "$lib/stores/app.svelte";
  import { exportOptions } from "$lib/stores/exportOptions.svelte";

  let preview = $state("");
  let loadingPreview = $state(false);
  let error = $state<string | null>(null);
  let previewRequestId = 0;
  // Which item lists are expanded is view state only; it resets with the page.
  let open = $state<Partial<Record<ExportCategory, boolean>>>({});

  const prefs = $derived(exportOptions.prefs);
  const highlighted = $derived(highlightMarkdown(preview));
  const size = $derived(preview ? previewSize(preview) : "");

  function rowItems(key: ExportCategory): ExportItem[] | null {
    return isPickable(key) ? visibleItems(exportOptions.items[key], prefs.onlyUnresolved) : null;
  }

  // Annotations and the checklist cannot be picked item by item, so their row
  // shows a plain total instead of picked/total.
  function rowCount(key: ExportCategory, items: ExportItem[] | null): string {
    if (items) return `${items.filter((i) => !exportOptions.isExcluded(i.id)).length}/${items.length}`;
    if (key === "annotations") {
      return String((app.snapshot?.ui_annotations ?? []).filter((a) => !(prefs.onlyUnresolved && a.stale)).length);
    }
    return String(app.snapshot?.ai?.checklist?.items.length ?? 0);
  }

  async function refreshPreview(opts = exportOptions.current) {
    const requestId = ++previewRequestId;
    loadingPreview = true;
    try {
      const next = await invoke<string>("export_review", { opts });
      if (requestId !== previewRequestId) return;
      preview = next;
      error = null;
    } catch (e) {
      if (requestId !== previewRequestId) return;
      error = String(e);
    } finally {
      if (requestId === previewRequestId) loadingPreview = false;
    }
  }

  async function saveToFile() {
    try {
      const target = await invoke<string>("export_review_to_file", { opts: exportOptions.current, path: "" });
      app.showToast("success", `Saved to ${target}`);
    } catch (e) {
      app.showToast("error", `Save failed: ${String(e)}`);
    }
  }

  async function copyReviewJson() {
    try {
      const body = await invoke<string>("read_review_json", { revisionId: null });
      await copyToClipboard(body);
      app.showToast("success", `Copied review.json (${body.length} bytes)`);
    } catch {
      app.showToast("error", "No review.json found for this review tab");
    }
  }

  // The preview follows the options and the review itself: a comment added
  // while this page is open shows up without touching a toggle.
  $effect(() => {
    if (app.mainView !== "export-review") return;
    const opts = exportOptions.current;
    void app.snapshot?.ai;
    void app.snapshot?.ui_annotations;
    void app.snapshot?.active_tab;
    void refreshPreview(opts);
  });
</script>

{#snippet check(on: boolean)}
  <span class="cb" aria-hidden="true" data-on={on}>
    <svg width="9" height="9" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="2"><path d="M2 5.2l2 2L8 3" /></svg>
  </span>
{/snippet}

<section class="ex flex-1 min-w-0 min-h-0 overflow-hidden" aria-label="Export review">
  <header class="ex-head">
    <button type="button" class="ex-btn ghost" onclick={() => app.setMainView("diff")}>‹ Back</button>
    <div class="min-w-0">
      <h2>Export review</h2>
      <p class="dim">Markdown for a PR description, a handoff, or an agent prompt.</p>
    </div>
    <span class="flex-1"></span>
    <span class="faint text-[12px]" aria-live="polite">{exportOptions.itemCount} items</span>
    <button type="button" class="ex-btn" onclick={saveToFile}>Save to file</button>
    <button type="button" class="ex-btn primary" onclick={() => void exportOptions.quickCopy()}>
      <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
        <path d="M5.5 5.5V3.25c0-.4.35-.75.75-.75h6.5c.4 0 .75.35.75.75v6.5c0 .4-.35.75-.75.75H10.5" />
        <rect x="2.5" y="5.5" width="8" height="8" rx=".75" />
      </svg>
      Copy markdown <span class="mono kbd-hint">⌘⇧C</span>
    </button>
  </header>

  <div class="ex-body">
    <div class="ex-opts">
      <div class="ex-sec">Include</div>
      {#each EXPORT_CATEGORIES as cat (cat.key)}
        {@const on = prefs.include[cat.key]}
        {@const items = rowItems(cat.key)}
        {@const expanded = !!open[cat.key]}
        <div class="ex-cat" class:off={!on}>
          <div class="ex-row">
            <button
              type="button"
              class="ex-cbx"
              role="checkbox"
              aria-checked={on}
              aria-label={cat.label}
              onclick={() => exportOptions.toggleCategory(cat.key)}
            >
              {@render check(on)}
            </button>
            <span class="ex-l">{cat.label}</span>
            <span class="faint mono text-[11px]">{rowCount(cat.key, items)}</span>
            {#if items && items.length > 0}
              <button
                type="button"
                class="chev"
                aria-expanded={expanded}
                aria-label="Pick {cat.label}"
                onclick={() => (open = { ...open, [cat.key]: !expanded })}
              >
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 6l4 4 4-4" /></svg>
              </button>
            {:else}
              <span class="chev-slot"></span>
            {/if}
          </div>
          {#if items && expanded && on}
            <div class="ex-items">
              {#each items as item (item.id)}
                {@const picked = !exportOptions.isExcluded(item.id)}
                <button
                  type="button"
                  class="ex-item"
                  role="checkbox"
                  aria-checked={picked}
                  title="{item.file}:{item.line ?? '—'} — {item.label}"
                  onclick={() => exportOptions.toggleItem(item.id)}
                >
                  {@render check(picked)}
                  <span class="mono faint">{itemLocation(item)}</span>
                  <span class="t">{item.label || "(empty)"}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/each}

      <div class="ex-sec mt-3">Filter</div>
      <button
        type="button"
        class="toggle"
        role="switch"
        aria-checked={prefs.onlyUnresolved}
        onclick={() => exportOptions.toggleOnlyUnresolved()}
      >
        <span class="sw"></span>Only unresolved
      </button>
      <p class="faint ex-help">
        Turn a category off to drop its section; open it to leave out single items. These choices are what Copy export uses everywhere.
      </p>

      <div class="ex-sec mt-3">Raw</div>
      <button type="button" class="ex-btn ghost self-start" onclick={copyReviewJson}>Copy review.json</button>
    </div>

    <div class="ex-prev">
      <div class="ex-prev-h">
        <span>Preview{loadingPreview ? " · refreshing…" : ""}</span>
        <span class="faint mono">{size}</span>
      </div>
      {#if error}
        <pre class="ex-md err">{error}</pre>
      {:else if preview}
        <!-- highlightMarkdown escapes the source before adding its spans. -->
        <!-- eslint-disable-next-line svelte/no-at-html-tags -- escaped in highlightMarkdown -->
        <pre class="ex-md">{@html highlighted}</pre>
      {:else}
        <pre class="ex-md faint">{loadingPreview ? "Loading…" : "No preview."}</pre>
      {/if}
    </div>
  </div>
</section>

<style>
  .ex {
    display: flex;
    flex-direction: column;
    background: var(--color-bg);
    font-size: 13px;
  }
  .dim {
    color: var(--color-fg-2);
  }
  .faint {
    color: var(--color-fg-3);
  }
  .ex-head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--color-hairline);
    flex-wrap: wrap;
  }
  .ex-head h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--color-fg);
  }
  .ex-head p {
    margin: 2px 0 0;
    font-size: 12px;
  }
  .ex-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--color-border);
    background: var(--color-card);
    color: var(--color-fg-2);
    font-weight: 500;
    white-space: nowrap;
  }
  .ex-btn:hover {
    border-color: color-mix(in srgb, var(--color-fg) 25%, transparent);
  }
  .ex-btn.primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
    border-color: transparent;
  }
  .ex-btn.ghost {
    height: 26px;
    border-color: transparent;
    background: none;
  }
  .ex-btn.ghost:hover {
    background: var(--color-hover);
    color: var(--color-fg);
  }
  .kbd-hint {
    opacity: 0.6;
    font-size: 10.5px;
  }
  .ex-body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 320px minmax(0, 1fr);
  }
  .ex-opts {
    border-right: 1px solid var(--color-hairline);
    overflow: auto;
    padding: 14px 14px 24px;
    display: grid;
    gap: 4px;
    align-content: start;
  }
  .ex-sec {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-fg-3);
    padding: 2px 4px 4px;
  }
  .ex-cat {
    border: 1px solid var(--color-hairline);
    border-radius: 8px;
    background: var(--color-card);
  }
  .ex-cat.off {
    background: none;
  }
  .ex-cat.off .ex-l {
    color: var(--color-fg-3);
  }
  .ex-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 6px 10px;
  }
  .ex-l {
    flex: 1;
    font-weight: 500;
    color: var(--color-fg);
  }
  .ex-cbx {
    display: inline-grid;
  }
  .cb {
    width: 14px;
    height: 14px;
    border-radius: 4px;
    border: 1.5px solid var(--color-border);
    display: inline-grid;
    place-items: center;
    flex: none;
    background: var(--color-card);
    color: var(--color-card);
  }
  .cb svg {
    opacity: 0;
  }
  .cb[data-on="true"] {
    background: var(--color-add-fg);
    border-color: var(--color-add-fg);
  }
  .cb[data-on="true"] svg {
    opacity: 1;
  }
  .chev,
  .chev-slot {
    width: 22px;
    height: 22px;
    flex: none;
  }
  .chev {
    display: grid;
    place-items: center;
    border-radius: 5px;
    color: var(--color-fg-3);
  }
  .chev:hover {
    background: var(--color-hover);
  }
  .chev svg {
    transition: transform 0.15s;
  }
  .chev[aria-expanded="false"] svg {
    transform: rotate(-90deg);
  }
  .ex-items {
    border-top: 1px solid var(--color-hairline);
    padding: 4px;
    display: grid;
    gap: 1px;
  }
  .ex-item {
    display: grid;
    grid-template-columns: 14px auto minmax(0, 1fr);
    gap: 8px;
    align-items: center;
    padding: 5px 6px;
    border-radius: 5px;
    text-align: left;
    font-size: 12px;
    color: var(--color-fg-2);
  }
  .ex-item:hover {
    background: var(--color-hover);
  }
  .ex-item .mono {
    font-size: 10.5px;
    white-space: nowrap;
  }
  .ex-item .t {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ex-item[aria-checked="false"] .t {
    color: var(--color-fg-3);
    text-decoration: line-through;
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--color-fg-2);
    justify-self: start;
    padding: 2px 4px;
  }
  .sw {
    width: 26px;
    height: 16px;
    border-radius: 8px;
    background: var(--color-border);
    position: relative;
    transition: background 0.15s;
  }
  .sw::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--color-card);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--color-bg) 60%, transparent);
    transition: transform 0.15s;
  }
  .toggle[aria-checked="true"] .sw {
    background: var(--color-accent);
  }
  .toggle[aria-checked="true"] .sw::after {
    transform: translateX(10px);
  }
  .ex-help {
    font-size: 11.5px;
    margin: 10px 0 0;
    line-height: 1.5;
    padding: 0 4px;
  }
  .ex-prev {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 14px 20px 20px;
  }
  .ex-prev-h {
    display: flex;
    justify-content: space-between;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-fg-3);
    padding-bottom: 8px;
  }
  .ex-prev-h .mono {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
    font-size: 11px;
  }
  .ex-md {
    flex: 1;
    margin: 0;
    overflow: auto;
    padding: 16px 18px;
    border: 1px solid var(--color-hairline);
    border-radius: 10px;
    background: var(--color-card);
    font-family: "JetBrains Mono", monospace;
    font-size: 12px;
    line-height: 1.65;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--color-fg-2);
  }
  .ex-md.err {
    color: var(--color-del-fg);
  }
  .ex-md :global(.mh) {
    color: var(--color-fg);
    font-weight: 600;
  }
  .ex-md :global(.mq) {
    color: var(--color-fg-2);
  }
  .ex-md :global(.mc) {
    color: var(--color-add-fg);
  }
  .ex-md :global(.mcode) {
    color: var(--color-syntax-string);
  }
  .ex-md :global(b) {
    color: var(--color-fg);
    font-weight: 600;
  }
  @media (max-width: 880px) {
    .ex {
      overflow: auto;
    }
    .ex-body {
      grid-template-columns: minmax(0, 1fr);
    }
    .ex-opts {
      border-right: 0;
      border-bottom: 1px solid var(--color-hairline);
    }
    .ex-md {
      min-height: 420px;
    }
  }
</style>
