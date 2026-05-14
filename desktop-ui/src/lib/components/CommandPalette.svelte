<script lang="ts">
  import { onMount, tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { openExportModal } from "$lib/components/ExportModal.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import { copyToClipboard } from "$lib/clipboard";

  interface CommandItem {
    id: string;
    label: string;
    description?: string;
    group: "Actions" | "Navigate" | "Files in this diff";
    kbd?: string;
    /** Action to run when the row is activated. */
    run: () => void;
    /** Filename to render for "Files" entries — uses mono font + diff stats. */
    file?: { path: string; additions: number; deletions: number };
  }

  let open = $state(false);
  let query = $state("");
  let selectedIdx = $state(0);
  let inputEl = $state<HTMLInputElement | null>(null);

  const snapshot = $derived(app.snapshot);

  function close() {
    open = false;
    query = "";
    selectedIdx = 0;
  }

  function buildItems(): CommandItem[] {
    const items: CommandItem[] = [
      {
        id: "comment-current-hunk",
        label: "Comment on current hunk",
        description: "Add a personal review note",
        group: "Actions",
        kbd: "c",
        run: () => {
          close();
          const snap = app.snapshot;
          if (!snap) return;
          const file = snap.files[snap.selected_file];
          if (!file) return;
          const hunk = file.hunks[snap.current_hunk ?? 0];
          if (!hunk) return;
          const ln = hunk.lines.find((l) => l.new_num !== null)?.new_num ?? hunk.new_start;
          diffSel.kind = "comment";
          diffSel.file = file.path;
          diffSel.start = ln;
          diffSel.end = ln;
        },
      },
      {
        id: "commit-staged",
        label: "Commit staged changes",
        description: "Open commit composer",
        group: "Actions",
        kbd: "⌘⏎",
        run: () => { close(); app.cmd("open_commit_composer"); },
      },
      {
        id: "export-review-copy",
        label: "Export review (copy)",
        description: "Open export modal — copy markdown to clipboard",
        group: "Actions",
        kbd: "⌘⇧E",
        run: () => { close(); openExportModal(); },
      },
      {
        id: "export-review-file",
        label: "Export review to file",
        description: "Write markdown to .er/export.md",
        group: "Actions",
        run: () => { close(); app.cmd("export_to_agent"); },
      },
      {
        id: "open-browser-view",
        label: browser.open ? "Close browser view" : "Open browser view",
        description: "Embedded iframe — click elements to annotate",
        group: "Actions",
        run: () => { close(); browser.toggleOpen(); },
      },
      {
        id: "next-unreviewed",
        label: "Jump to next unreviewed file",
        group: "Navigate",
        kbd: "U",
        run: () => { close(); app.cmd("jump_to_unreviewed"); },
      },
      {
        id: "refresh",
        label: "Refresh diff",
        group: "Navigate",
        kbd: "R",
        run: () => { close(); app.cmd("refresh_diff"); },
      },
      {
        id: "toggle-diff-view-mode",
        label: "Toggle diff view (unified/split)",
        description: `Currently: ${app.diffViewMode}`,
        group: "Navigate",
        kbd: "d",
        run: () => { close(); app.toggleDiffViewMode(); },
      },
      {
        id: "copy-logs",
        label: `Copy logs to clipboard (${app.logs.length})`,
        description: "All captured errors & warnings since launch",
        group: "Actions",
        run: () => {
          close();
          const text = app.dumpLogs() || "(no logs)";
          copyToClipboard(text)
            .then(() => app.pushLog("info", "clipboard", `Copied ${text.length} chars`))
            .catch(() => {});
        },
      },
      {
        id: "clear-logs",
        label: "Clear logs",
        group: "Actions",
        run: () => { close(); app.clearLogs(); },
      },
      {
        id: "toggle-left",
        label: "Toggle left panel",
        group: "Navigate",
        kbd: "[",
        run: () => { close(); app.togglePanel("left"); },
      },
      {
        id: "toggle-right",
        label: "Toggle right panel",
        group: "Navigate",
        kbd: "]",
        run: () => { close(); app.togglePanel("right"); },
      },
      {
        id: "run-ai-review-branch",
        label: "Run AI review (branch)",
        group: "Actions",
        run: () => { close(); app.cmd("run_ai_review", { scope: "branch" }); },
      },
      {
        id: "run-ai-review-staged",
        label: "Run AI review (staged only)",
        group: "Actions",
        run: () => { close(); app.cmd("run_ai_review", { scope: "staged" }); },
      },
      {
        id: "set-ai-model-opus",
        label: "Change AI model: Opus",
        group: "Actions",
        run: () => { close(); app.cmd("set_ai_model", { model: "opus" }); },
      },
      {
        id: "set-ai-model-sonnet",
        label: "Change AI model: Sonnet",
        group: "Actions",
        run: () => { close(); app.cmd("set_ai_model", { model: "sonnet" }); },
      },
      {
        id: "set-ai-model-haiku",
        label: "Change AI model: Haiku",
        group: "Actions",
        run: () => { close(); app.cmd("set_ai_model", { model: "haiku" }); },
      },
      {
        id: "toggle-terminal",
        label: terminal.open ? "Close terminal" : "Toggle terminal",
        description: "Bottom drawer shell at the active tab's repo root",
        group: "Actions",
        kbd: "`",
        run: () => { close(); terminal.toggle(); },
      },
    ];

    for (const wt of (snapshot?.worktrees ?? []).filter((w) => !w.is_current)) {
      items.push({
        id: `switch-worktree-${wt.path}`,
        label: `Switch worktree: ${wt.branch}`,
        group: "Navigate",
        run: () => { close(); app.cmd("switch_worktree", { path: wt.path }); },
      });
    }

    for (const [idx, file] of (snapshot?.files ?? []).entries()) {
      items.push({
        id: `file-${file.path}`,
        label: file.path,
        group: "Files in this diff",
        file: { path: file.path, additions: file.additions, deletions: file.deletions },
        run: () => { close(); app.cmd("select_file", { idx }); },
      });
    }
    return items;
  }

  /** Fuzzy match: query characters must appear in order, not necessarily contiguous. */
  function matches(label: string, q: string): boolean {
    if (!q) return true;
    const lower = label.toLowerCase();
    const lowerQ = q.toLowerCase();
    let qi = 0;
    for (let i = 0; i < lower.length && qi < lowerQ.length; i++) {
      if (lower[i] === lowerQ[qi]) qi++;
    }
    return qi === lowerQ.length;
  }

  const allItems = $derived(buildItems());
  const filtered = $derived(allItems.filter((item) => matches(item.label, query)));

  /** Group filtered items in mock display order. */
  const grouped = $derived(
    (["Actions", "Navigate", "Files in this diff"] as const).map((group) => ({
      group,
      items: filtered.filter((i) => i.group === group),
    })).filter((g) => g.items.length > 0),
  );

  /** Flat ordered list used for keyboard navigation indexing. */
  const flat = $derived(grouped.flatMap((g) => g.items));

  $effect(() => {
    // Reset selection when filter narrows past current index.
    if (selectedIdx >= flat.length) selectedIdx = 0;
  });

  function highlight(text: string, q: string): { match: string; rest: string }[] {
    if (!q) return [{ match: "", rest: text }];
    const lowerT = text.toLowerCase();
    const lowerQ = q.toLowerCase();
    const idx = lowerT.indexOf(lowerQ);
    if (idx === -1) return [{ match: "", rest: text }];
    return [
      { match: "", rest: text.slice(0, idx) },
      { match: text.slice(idx, idx + q.length), rest: "" },
      { match: "", rest: text.slice(idx + q.length) },
    ];
  }

  async function openPalette() {
    open = true;
    selectedIdx = 0;
    query = "";
    await tick();
    inputEl?.focus();
  }

  function onKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === "k") {
      e.preventDefault();
      open ? close() : openPalette();
      return;
    }
    if (!open) return;
    if (e.key === "Escape") { e.preventDefault(); close(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); selectedIdx = Math.min(selectedIdx + 1, flat.length - 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); selectedIdx = Math.max(selectedIdx - 1, 0); }
    else if (e.key === "Enter") { e.preventDefault(); flat[selectedIdx]?.run(); }
  }

  onMount(() => {
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  function basename(path: string): string {
    const segments = path.split("/").filter(Boolean);
    return segments.length > 2 ? `…/${segments.slice(-2).join("/")}` : path;
  }
</script>

{#if open}
  <!-- Backdrop -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-[100] bg-black/50"
    style="backdrop-filter: blur(2px);"
    onclick={close}
  ></div>

  <!-- Palette -->
  <div class="fixed left-1/2 -translate-x-1/2 top-[12vh] w-[640px] z-[101] rounded-xl bg-card border border-border shadow-2xl overflow-hidden">
    <div class="flex items-center gap-3 px-4 py-3 border-b border-hairline">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#5e5e5e" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
      <input
        bind:this={inputEl}
        bind:value={query}
        class="flex-1 bg-transparent outline-none text-base placeholder:text-muted"
        placeholder="Type a command or jump to file…"
      />
      <span class="kbd">esc</span>
    </div>

    <div class="max-h-[60vh] overflow-y-auto py-1">
      {#each grouped as group (group.group)}
        <div class="px-4 pt-2 pb-1 text-[10px] uppercase tracking-wider text-muted">{group.group}</div>
        {#each group.items as item, _localIdx (item.id)}
          {@const globalIdx = flat.indexOf(item)}
          {@const isActive = globalIdx === selectedIdx}
          <button
            onclick={item.run}
            onmouseenter={() => (selectedIdx = globalIdx)}
            class="w-full flex items-center gap-3 px-4 py-2 text-left transition-colors {isActive ? 'bg-hover' : 'hover:bg-hover'}"
          >
            {#if item.file}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#5e5e5e" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
              <div class="flex-1 mono text-[13px] text-fg-2 truncate">{basename(item.file.path)}</div>
              {#if item.file.additions > 0}
                <span class="text-[10px] text-add-fg mono">+{item.file.additions}</span>
              {/if}
              {#if item.file.deletions > 0}
                <span class="text-[10px] text-del-fg mono">−{item.file.deletions}</span>
              {/if}
            {:else}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke={isActive ? "#ff6a3d" : "#999"} stroke-width="2"><circle cx="12" cy="12" r="9"/></svg>
              <div class="flex-1">
                <div class="text-sm {isActive ? 'text-fg' : 'text-fg-2'}">
                  {#each highlight(item.label, query) as part}{#if part.match}<span class="text-accent font-medium">{part.match}</span>{:else}{part.rest}{/if}{/each}
                </div>
                {#if item.description}
                  <div class="text-[11px] text-muted">{item.description}</div>
                {/if}
              </div>
              {#if item.kbd}
                <span class="kbd">{item.kbd}</span>
              {/if}
            {/if}
          </button>
        {/each}
      {/each}
      {#if flat.length === 0}
        <div class="px-4 py-6 text-center text-sm text-muted">No matches</div>
      {/if}
    </div>

    <div class="border-t border-hairline px-4 py-2 flex items-center gap-3 text-[11px] text-muted">
      <span class="flex items-center gap-1"><span class="kbd">↑</span><span class="kbd">↓</span> nav</span>
      <span class="flex items-center gap-1"><span class="kbd">⏎</span> run</span>
      <span class="flex items-center gap-1"><span class="kbd">⇥</span> autocomplete</span>
      <span class="ml-auto flex items-center gap-1">
        <span>by</span>
        <span class="kbd">:</span><span>commands</span>
        <span class="kbd">/</span><span>files</span>
        <span class="kbd">@</span><span>symbols</span>
      </span>
    </div>
  </div>
{/if}
