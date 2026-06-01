<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import { commandPalette } from "$lib/stores/commandPalette.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
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

  let query = $state("");
  let selectedIdx = $state(0);
  let inputEl = $state<HTMLInputElement | null>(null);

  const snapshot = $derived(app.snapshot);

  function close() {
    commandPalette.close();
    query = "";
    selectedIdx = 0;
  }

  function openExportReviewView() {
    app.setMainView("export-review");
    if (browser.layout === "fullscreen") void browser.setLayout("hidden");
  }

  function buildItems(): CommandItem[] {
    const mode = snapshot?.mode;
    const reviewScope =
      mode === "branch" || mode === "unstaged" || mode === "staged" ? mode : null;
    const scopeDescription =
      mode === "branch"
        ? "All changes vs base"
        : mode === "unstaged"
          ? "Working tree changes"
          : mode === "staged"
            ? "Staged changes only"
            : "Switch to All changes, Unstaged, or Staged";
    const hasReviewJson = snapshot?.ai?.has_review_json ?? false;
    const eligibleCommentCount = snapshot?.ai?.eligible_comment_count ?? 0;
    const validateAvailable = hasReviewJson || eligibleCommentCount > 0;
    const validateDescription = !reviewScope
      ? scopeDescription
      : !validateAvailable
        ? "Run General review or add GitHub comments first"
        : hasReviewJson && eligibleCommentCount > 0
          ? `Re-anchor review + ${eligibleCommentCount} comment(s)`
          : eligibleCommentCount > 0
            ? `Re-anchor ${eligibleCommentCount} GitHub comment(s)`
            : "Re-anchor AI review findings";

    const items: CommandItem[] = [
      {
        id: "open-in-vscode",
        label: "Open in VS Code",
        description: "Open selected file at current hunk (local checkout only)",
        group: "Actions",
        kbd: "e",
        run: () => {
          close();
          void invoke<{ kind: string; target: string }>("open_in_vscode").then((r) => {
            if (r.kind === "needs_checkout") app.showToast("info", r.target);
          }).catch((e) => app.showToast("error", `VS Code: ${e}`));
        },
      },
      {
        id: "export-review-copy",
        label: "Export review",
        description: "Open export view for copy, save, and preview",
        group: "Actions",
        kbd: "⌘⇧E",
        run: () => { close(); openExportReviewView(); },
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
        label: browser.open ? "Cycle browser layout" : "Open browser (split)",
        description: "Per-tab embedded browser — ⌘B cycles hidden → split → fullscreen",
        group: "Actions",
        kbd: "⌘B",
        run: () => { close(); void (browser.open ? browser.cycleLayout() : browser.setLayout("split")); },
      },
      {
        id: "refresh",
        label: "Refresh diff",
        group: "Navigate",
        kbd: "R",
        run: () => { close(); app.cmd("refresh_diff"); },
      },
      {
        id: "force-refresh",
        label: "Force refresh diff",
        description: "Re-fetch PR head and base from remote",
        group: "Navigate",
        kbd: "⌘R",
        run: () => { close(); app.cmd("force_refresh_diff"); },
      },
      {
        id: "open-settings",
        label: "Open settings",
        section: "App",
        keywords: ["config", "preferences"],
        run: () => { close(); app.setMainView("settings"); },
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
      ...(reviewScope
        ? [
            {
              id: "run-ai-review-current",
              label: "Run AI review (current view)",
              description: scopeDescription,
              group: "Actions" as const,
              run: () => {
                close();
                void app.cmd("run_ai_review", { scope: reviewScope });
              },
            },
            ...(validateAvailable
              ? [
                  {
                    id: "run-ai-validate-current",
                    label: "Validate / re-anchor (current view)",
                    description: validateDescription,
                    group: "Actions" as const,
                    run: () => {
                      close();
                      void app.cmd("run_ai_validate", { scope: reviewScope });
                    },
                  },
                ]
              : []),
          ]
        : []),
      {
        id: "toggle-terminal",
        label: terminal.open ? "Close terminal" : "Toggle terminal",
        description: "Bottom drawer shell at the active tab's repo root",
        group: "Actions",
        kbd: "`",
        run: () => { close(); terminal.toggle(); },
      },
    ];

    for (const file of snapshot?.files ?? []) {
      items.push({
        id: `file-${file.path}`,
        label: file.path,
        group: "Files in this diff",
        file: { path: file.path, additions: file.additions, deletions: file.deletions },
        run: () => { close(); app.cmd("select_file", { idx: file.source_index }); },
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

  function openPalette() {
    selectedIdx = 0;
    query = "";
    commandPalette.show();
  }

  function onModalKeydown(e: KeyboardEvent) {
    if (!commandPalette.open) return;
    if (e.key === "Escape") { e.preventDefault(); close(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); selectedIdx = Math.min(selectedIdx + 1, flat.length - 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); selectedIdx = Math.max(selectedIdx - 1, 0); }
    else if (e.key === "Enter") { e.preventDefault(); flat[selectedIdx]?.run(); }
  }

  $effect(() => {
    function onGlobalKeydown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        if (commandPalette.open) close();
        else openPalette();
      }
    }
    window.addEventListener("keydown", onGlobalKeydown);
    return () => window.removeEventListener("keydown", onGlobalKeydown);
  });

  function basename(path: string): string {
    const segments = path.split("/").filter(Boolean);
    return segments.length > 2 ? `…/${segments.slice(-2).join("/")}` : path;
  }
</script>

<ModalShell
  open={commandPalette.open}
  ariaLabel="Command palette"
  onClose={close}
  onKeydown={onModalKeydown}
  closeOnEscape={false}
  focusSelector="input"
  backdropClass="fixed inset-0 z-[100] bg-black/50"
  panelClass="fixed left-1/2 -translate-x-1/2 top-[12vh] w-[640px] z-[101] rounded-xl bg-card border border-border shadow-2xl overflow-hidden outline-none"
>
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
</ModalShell>
