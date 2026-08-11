<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import { commandPalette } from "$lib/stores/commandPalette.svelte";
  import { openPrUrlModal } from "$lib/stores/prUrlModal.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import { reviewScopeFromMode, scopeDescriptionFromMode } from "$lib/reviewScope";
  import { openAiReviewFilesModal } from "$lib/components/AiReviewFilesModal.svelte";
  import { openProfessorFocusModal } from "$lib/components/ProfessorFocusModal.svelte";
  import ReviewerPickerList from "$lib/components/ReviewerPickerList.svelte";
  import type { AiProviderInfo } from "$lib/types";

  type Group = "Actions" | "Navigate" | "View & Layout" | "AI" | "PR" | "Files in this diff";

  interface CommandItem {
    id: string;
    label: string;
    description?: string;
    group: Group;
    kbd?: string;
    /** Action to run when the row is activated (leaf items). */
    run: () => void;
    /** Filename to render for "Files" entries — uses mono font + diff stats. */
    file?: { path: string; additions: number; deletions: number };
    /** Sub-items shown in a second-level list when this item is activated. */
    submenuItems?: CommandItem[];
    /** Render a custom submenu body instead of a list (currently: reviewer picker). */
    view?: "reviewers";
    /** Label of the submenu this item lives in (shown when surfaced by search). */
    submenuOf?: string;
  }

  let query = $state("");
  let selectedIdx = $state(0);
  let inputEl = $state<HTMLInputElement | null>(null);
  let listEl = $state<HTMLDivElement | null>(null);

  /** Stack of submenu entries — the top entry is the currently-open submenu.
   *  Each entry remembers the selected index + query of the view it was opened
   *  from, so ← returns to the same active item. */
  let submenuStack = $state<{ item: CommandItem; selectedIdx: number; query: string }[]>([]);

  // ── AI submenu state (mirrors the old ⌘A AI action palette) ──────────────
  let aiProviders = $state<AiProviderInfo[]>([]);
  let reviewerSelection = $state<Set<string>>(new Set());
  let reviewerHighlight = $state(0);
  let reviewerPickerRef = $state<{ moveHighlight: (d: number) => void; toggleHighlighted: () => void } | null>(null);

  const snapshot = $derived(app.snapshot);

  function close() {
    commandPalette.close();
    query = "";
    selectedIdx = 0;
    submenuStack = [];
    reviewerSelection = new Set();
  }

  /** Close the palette from a submenu action. Clears the submenu stack so the
   *  next open (via ⌘K or the TabStrip/sidebar buttons) lands at the root. */
  function closeKeepSubmenu() {
    commandPalette.close();
    query = "";
    selectedIdx = 0;
    submenuStack = [];
    reviewerSelection = new Set();
  }

  /** Close the palette, let it paint, then run a heavy (IPC / snapshot) action. */
  async function dismissAndRun(fn: () => void | Promise<void>) {
    closeKeepSubmenu();
    await Promise.resolve();
    if (typeof requestAnimationFrame === "function") {
      await new Promise<void>((r) => requestAnimationFrame(() => r()));
    }
    await fn();
  }

  /** Instant local UI — no IPC. Close immediately and run. */
  function dismissLocal(fn: () => void) {
    close();
    fn();
  }

  function openExportReviewView() {
    app.setMainView("export-review");
    if (browser.layout === "fullscreen") void browser.setLayout("hidden");
  }

  // ── AI actions (previously the ⌘A AI action palette, now inline) ─────────
  function buildAiItems(): CommandItem[] {
    const mode = snapshot?.mode;
    const reviewScope = reviewScopeFromMode(mode);
    const scopeDescription = scopeDescriptionFromMode(mode);
    const hasReviewJson = snapshot?.ai?.has_review_json ?? false;
    const eligibleCommentCount = snapshot?.ai?.eligible_comment_count ?? 0;
    const validateAvailable = hasReviewJson || eligibleCommentCount > 0;
    const tourAvailable = snapshot?.tour?.available ?? false;
    const runningCommands = (snapshot?.agent_commands ?? []).filter((c) => c.status === "running");
    const activeAiLabel = snapshot?.active_ai_label ?? "";
    const validateDescription = !reviewScope
      ? scopeDescription
      : !validateAvailable
        ? "Run General review or add GitHub comments first"
        : hasReviewJson && eligibleCommentCount > 0
          ? `Re-anchor review + ${eligibleCommentCount} comment(s)`
          : eligibleCommentCount > 0
            ? `Re-anchor ${eligibleCommentCount} GitHub comment(s)`
            : "Re-anchor AI review findings";

    const guard = (fn: () => void) => () => { if (!reviewScope) return; fn(); };

    return [
      {
        id: "ai-triage",
        label: "Triage branch",
        description: reviewScope
          ? "Fast scan — first impression and review routing (default model, low effort)"
          : "Not available in this view",
        group: "AI" as const,
        kbd: "t",
        run: guard(() => { void dismissAndRun(() => app.cmd("run_ai_triage_review", { scope: reviewScope })); }),
      },
      {
        id: "ai-run-review",
        label: "Run review",
        description: reviewScope
          ? `General review only — risk, order, checklist, summary (${scopeDescription.toLowerCase()})`
          : "Not available in this view",
        group: "AI" as const,
        kbd: "r",
        run: guard(() => { void dismissAndRun(() => app.cmd("run_ai_review", { scope: reviewScope })); }),
      },
      {
        id: "ai-run-reviewers",
        label: "Run reviewers…",
        description: reviewScope
          ? "Multi-select General, experts, and Professor"
          : "Not available in this view",
        group: "AI" as const,
        kbd: "v",
        view: "reviewers" as const,
        run: () => {},
      },
      {
        id: "ai-professor",
        label: "Professor",
        description: reviewScope
          ? "Learn what this diff implements (not a code review)"
          : "Not available in this view",
        group: "AI" as const,
        kbd: "p",
        run: guard(() => {
          if (!reviewScope) return;
          dismissLocal(() => openProfessorFocusModal(reviewScope, ["professor"], []));
        }),
      },
      {
        id: "ai-tour",
        label: tourAvailable ? "Regenerate tour" : "Generate tour",
        description: reviewScope
          ? "Group the diff into a guided walkthrough (pillars) for the Guide tab"
          : "Not available in this view",
        group: "AI" as const,
        kbd: "g",
        run: guard(() => { void dismissAndRun(() => app.cmd("generate_tour")); }),
      },
      {
        id: "ai-validate",
        label: "Validate / re-anchor",
        description: validateDescription,
        group: "AI" as const,
        kbd: "l",
        run: guard(() => {
          if (!validateAvailable) return;
          void dismissAndRun(() => app.cmd("run_ai_validate", { scope: reviewScope }));
        }),
      },
      {
        id: "ai-select-files",
        label: "Review select files",
        description: reviewScope
          ? `Choose files and reviewers (${scopeDescription.toLowerCase()})`
          : "Not available in this view",
        group: "AI" as const,
        kbd: "s",
        run: guard(() => { dismissLocal(() => openAiReviewFilesModal()); }),
      },
      {
        id: "ai-open-output",
        label: "Open agent output",
        description: runningCommands.length > 0
          ? `${runningCommands.length} command(s) running — view live output`
          : "View the agent log from the last run",
        group: "AI" as const,
        kbd: "o",
        run: () => { dismissLocal(() => { app.setMainView("agent-output"); }); },
      },
      {
        id: "ai-copy-context",
        label: "Copy review context",
        description: "Export current diff context to clipboard",
        group: "AI" as const,
        kbd: "c",
        run: () => { void dismissAndRun(() => app.cmd("export_to_agent")); },
      },
      {
        id: "ai-provider-model",
        label: "Change provider / model",
        description: activeAiLabel ? `Currently: ${activeAiLabel}` : "Select AI provider and model",
        group: "AI" as const,
        kbd: "m",
        run: () => { void openProviderPicker(); },
      },
    ];
  }

  // ── Provider / model nested picker (mirrors the old palette subviews) ────
  function providerItems(): CommandItem[] {
    return aiProviders.map((p) => ({
      id: `provider-${p.id}`,
      label: p.label,
      description: p.models.length > 0
        ? `${p.models.length} model${p.models.length === 1 ? "" : "s"}${p.is_selected ? " · active" : ""}`
        : p.is_selected ? "active" : "no model presets",
      group: "AI" as const,
      run: () => {
        if (p.models.length === 0) {
          void dismissAndRun(() =>
            app.cmd("set_ai_selection", { providerId: p.id, modelId: null, persist: false }),
          );
        }
      },
      submenuItems: p.models.length > 0
        ? p.models.map((m) => ({
            id: `model-${p.id}-${m.id}`,
            label: m.label,
            description: m.is_selected ? "currently selected" : "",
            group: "AI" as const,
            run: () => {
              void dismissAndRun(() =>
                app.cmd("set_ai_selection", { providerId: p.id, modelId: m.id, persist: false }),
              );
            },
          }))
        : undefined,
    }));
  }

  async function openProviderPicker() {
    try {
      const list = await invoke<AiProviderInfo[]>("list_ai_providers");
      aiProviders = list;
      if (list.length === 0) {
        app.showToast("error", "No [ai_hub] providers configured — add one in Settings → AI Hub");
        return;
      }
      pushSubmenu({
        id: "ai-providers",
        label: "Select provider",
        group: "AI" as const,
        run: () => {},
        submenuItems: providerItems(),
      });
    } catch (e) {
      app.showToast("error", `list_ai_providers: ${e}`);
    }
  }

  async function runSelectedReviewers() {
    const mode = snapshot?.mode;
    const scope = reviewScopeFromMode(mode);
    if (!scope || reviewerSelection.size === 0) return;
    const kinds = [...reviewerSelection];
    if (kinds.includes("professor")) {
      dismissLocal(() => openProfessorFocusModal(scope, kinds, []));
      return;
    }
    void dismissAndRun(() =>
      app.cmd("run_ai_scoped_review", {
        scope,
        paths: [],
        reviewerKinds: kinds,
        focusPrompt: null,
      }),
    );
  }

  // ── Root menu containers ──────────────────────────────────────────────────
  function buildItems(): CommandItem[] {
    const mode = snapshot?.mode;
    const reviewScope = reviewScopeFromMode(mode);
    const scopeDescription = scopeDescriptionFromMode(mode);

    const actionsItems: CommandItem[] = [
      {
        id: "open-in-vscode",
        label: "Open in VS Code",
        description: "Open selected file at current hunk (local checkout only)",
        group: "Actions",
        kbd: "e",
        run: () => {
          void dismissAndRun(() =>
            invoke<{ kind: string; target: string }>("open_in_vscode").then((r) => {
              if (r.kind === "needs_checkout") app.showToast("info", r.target);
            }).catch((e) => app.showToast("error", `VS Code: ${e}`)),
          );
        },
      },
      {
        id: "export-review-copy",
        label: "Export review",
        description: "Open export view for copy, save, and preview",
        group: "Actions",
        kbd: "⌘⇧E",
        run: () => { dismissLocal(() => { openExportReviewView(); }); },
      },
      {
        id: "export-review-file",
        label: "Export review to file",
        description: "Write markdown to .er/export.md",
        group: "Actions",
        run: () => { void dismissAndRun(() => app.cmd("export_to_agent")); },
      },
      {
        id: "copy-logs",
        label: `Copy logs to clipboard (${app.logs.length})`,
        description: "All captured errors & warnings since launch",
        group: "Actions",
        run: () => {
          dismissLocal(() => {
            const text = app.dumpLogs() || "(no logs)";
            void copyToClipboard(text)
              .then(() => app.pushLog("info", "clipboard", `Copied ${text.length} chars`))
              .catch(() => {});
          });
        },
      },
      {
        id: "clear-logs",
        label: "Clear logs",
        group: "Actions",
        run: () => { dismissLocal(() => { app.clearLogs(); }); },
      },
      {
        id: "open-settings",
        label: "Open settings",
        group: "Actions",
        run: () => { dismissLocal(() => { app.setMainView("settings"); }); },
      },
    ];

    const navigateItems: CommandItem[] = [
      {
        id: "refresh",
        label: "Refresh diff",
        group: "Navigate",
        kbd: "R",
        run: () => { void dismissAndRun(() => app.cmd("refresh_diff")); },
      },
      {
        id: "force-refresh",
        label: "Force refresh diff",
        description: "Re-fetch PR head and base from remote",
        group: "Navigate",
        kbd: "⌘R",
        run: () => { void dismissAndRun(() => app.cmd("force_refresh_diff")); },
      },
      {
        id: "toggle-terminal",
        label: terminal.open ? "Hide terminal" : "Show terminal",
        description: "Bottom drawer shell at the active tab's repo root",
        group: "Navigate",
        kbd: "`",
        run: () => { dismissLocal(() => { terminal.toggle(); }); },
      },
    ];

    const viewItems: CommandItem[] = [
      {
        id: "toggle-diff-view-mode",
        label: "Toggle diff view (unified/split)",
        description: `Currently: ${app.diffViewMode}`,
        group: "View & Layout",
        kbd: "d",
        run: () => { dismissLocal(() => { app.toggleDiffViewMode(); }); },
      },
      {
        id: "open-browser-view",
        label: browser.open ? "Cycle browser layout" : "Open browser (split)",
        description: "Per-tab embedded browser — ⌘B cycles hidden → split → fullscreen",
        group: "View & Layout",
        kbd: "⌘B",
        run: () => {
          dismissLocal(() => {
            void (browser.open ? browser.cycleLayout() : browser.setLayout("split"));
          });
        },
      },
      {
        id: "toggle-left",
        label: "Toggle left panel",
        group: "View & Layout",
        kbd: "[",
        run: () => { dismissLocal(() => { app.togglePanel("left"); }); },
      },
      {
        id: "toggle-right",
        label: "Toggle right panel",
        group: "View & Layout",
        kbd: "]",
        run: () => { dismissLocal(() => { app.togglePanel("right"); }); },
      },
    ];

    const prItems: CommandItem[] = [
      {
        id: "open-pr-url",
        label: "Open PR by URL",
        description: "Paste a GitHub PR link to open it",
        group: "PR",
        kbd: "⌘⇧O",
        run: () => { dismissLocal(() => { openPrUrlModal(); }); },
      },
      {
        id: "open-arena",
        label: "Open AI review arena",
        description: "Compare multiple reviewers on the current diff",
        group: "PR",
        run: () => { dismissLocal(() => { arena.openLauncher(); }); },
      },
    ];

    const fileItems: CommandItem[] = (snapshot?.files ?? []).map((file) => ({
      id: `file-${file.path}`,
      label: file.path,
      group: "Files in this diff" as const,
      file: { path: file.path, additions: file.additions, deletions: file.deletions },
      run: () => {
        void dismissAndRun(() => {
          // Mirror FileTree.jumpToFile: select + scroll the flat diff to the file.
          void diffNav.scrollToFile(file.path);
          return app.cmd("select_file", { idx: file.source_index });
        });
      },
    }));

    const aiItems = buildAiItems();

    const containers: CommandItem[] = [
      { id: "menu-actions", label: "Actions", description: "Export, settings, logs", group: "Actions", kbd: "s", run: () => {}, submenuItems: actionsItems },
      { id: "menu-navigate", label: "Navigate", description: "Refresh, terminal, agent output", group: "Navigate", kbd: "n", run: () => {}, submenuItems: navigateItems },
      { id: "menu-view", label: "View & Layout", description: "Diff mode, browser, panels", group: "View & Layout", kbd: "v", run: () => {}, submenuItems: viewItems },
      { id: "menu-ai", label: "AI", description: reviewScope ? "Reviews, triage, Professor, model" : "Model picker available; reviews need a diff view", group: "AI", kbd: "a", run: () => {}, submenuItems: aiItems },
      { id: "menu-pr", label: "PR", description: "Open by URL, arena", group: "PR", kbd: "p", run: () => {}, submenuItems: prItems },
      ...(fileItems.length > 0
        ? [{ id: "menu-files", label: "Files in this diff", description: `${fileItems.length} files`, group: "Files in this diff" as const, kbd: "f", run: () => {}, submenuItems: fileItems }]
        : []),
    ];

    return containers;
  }

  /** Flatten all items, including those nested in submenus, for search. */
  function flattenAll(items: CommandItem[], parent?: CommandItem): CommandItem[] {
    const out: CommandItem[] = [];
    for (const item of items) {
      if (parent) out.push({ ...item, submenuOf: parent.label });
      else out.push(item);
      if (item.submenuItems) out.push(...flattenAll(item.submenuItems, item));
    }
    return out;
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

  /** Push a submenu (a parent entry) onto the stack and reset nav/focus.
   *  Records the current view's selected index + query so ← restores them. */
  function pushSubmenu(parent: CommandItem) {
    submenuStack = [...submenuStack, { item: parent, selectedIdx, query }];
    query = "";
    selectedIdx = 0;
    if (parent.view === "reviewers") reviewerSelection = new Set();
    queueMicrotask(() => inputEl?.focus());
  }

  /** Open a list item: push a submenu if it has one, otherwise run it. */
  function openItem(item: CommandItem) {
    if (item.submenuItems || item.view) pushSubmenu(item);
    else item.run();
  }

  /** Pop the current submenu (Esc or ←). Restores the position (selected item
   *  + filter) of the view we came from, at any nesting level. */
  function goBack() {
    const prev = submenuStack[submenuStack.length - 1];
    submenuStack = submenuStack.slice(0, -1);
    query = prev?.query ?? "";
    selectedIdx = prev?.selectedIdx ?? 0;
    queueMicrotask(() => inputEl?.focus());
  }

  /** The top item of the submenu stack, or null when showing the root list. */
  const activeSubmenu = $derived(submenuStack[submenuStack.length - 1]?.item ?? null);

  /** Root items: the menu containers (Actions / Navigate / … / Files).
   *  Only build while open — when closed, `$effect`s below still subscribe to
   *  `navList`, and rebuilding the full file list on every poll snapshot was
   *  free work that amplified ⌘K action jank. */
  const allRootItems = $derived(commandPalette.open ? buildItems() : []);

  /** Flatten containers + their leaves for search (leaves get a "in <menu>" hint). */
  const searchableItems = $derived(
    commandPalette.open ? flattenAll(allRootItems) : [],
  );

  /** When searching, surface nested actions inline with their submenu hint. */
  const searchResults = $derived(
    query.trim() === ""
      ? []
      : searchableItems.filter((item) => matches(item.label, query)),
  );

  /** Root list: the menu containers, in order. */
  const flat = $derived(allRootItems);

  /** Single-letter keybinds for the current view's items (root containers or
   *  the active submenu's actions), e.g. a → AI, t → Triage, r → Run review. */
  const viewKeybinds = $derived.by<Map<string, CommandItem>>(() => {
    const m = new Map<string, CommandItem>();
    for (const item of navList) {
      if (item.kbd) m.set(item.kbd.toLowerCase(), item);
    }
    return m;
  });

  /** Items shown in a submenu (filtered by the query). */
  const filtered = $derived(
    activeSubmenu
      ? (activeSubmenu.submenuItems ?? []).filter((item) => matches(item.label, query))
      : searchResults,
  );

  /** The list navigated by arrows/Enter in the current view. */
  const navList = $derived(
    activeSubmenu ? filtered : (query.trim() !== "" ? searchResults : flat),
  );

  $effect(() => {
    if (!commandPalette.open) return;
    // Reset selection when filter narrows past current index.
    if (selectedIdx >= navList.length) selectedIdx = 0;
  });

  /** Keep the active row in view when keyboard-navigating. */
  $effect(() => {
    if (!commandPalette.open) return;
    void selectedIdx;
    void navList;
    if (!listEl) return;
    const active = listEl.querySelector<HTMLElement>('[data-active="true"]');
    active?.scrollIntoView({ block: "nearest" });
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
    submenuStack = [];
    reviewerSelection = new Set();
    commandPalette.show();
  }

  function onModalKeydown(e: KeyboardEvent) {
    if (!commandPalette.open) return;

    if (activeSubmenu?.view === "reviewers") {
      if (e.key === "Escape") { e.preventDefault(); goBack(); return; }
      if (e.key === "ArrowDown") { e.preventDefault(); reviewerPickerRef?.moveHighlight(1); return; }
      if (e.key === "ArrowUp") { e.preventDefault(); reviewerPickerRef?.moveHighlight(-1); return; }
      if (e.key === " ") { e.preventDefault(); reviewerPickerRef?.toggleHighlighted(); return; }
      if (e.key === "Enter") { e.preventDefault(); if (reviewerSelection.size > 0) void runSelectedReviewers(); return; }
      return;
    }

    if (e.key === "Escape") {
      e.preventDefault();
      if (activeSubmenu) goBack();
      else close();
    }
    else if (e.key === "ArrowLeft" && activeSubmenu) {
      e.preventDefault();
      goBack();
    }
    else if (e.key === "ArrowRight") {
      e.preventDefault();
      const item = navList[selectedIdx];
      if (item && (item.submenuItems || item.view)) openItem(item);
    }
    else if (e.key === "ArrowDown") { e.preventDefault(); selectedIdx = Math.min(selectedIdx + 1, navList.length - 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); selectedIdx = Math.max(selectedIdx - 1, 0); }
    else if (e.key === "Enter") { e.preventDefault(); const item = navList[selectedIdx]; if (item) openItem(item); }
    else if (
      !e.metaKey && !e.ctrlKey && !e.altKey &&
      e.key.length === 1 && /^[a-zA-Z]$/.test(e.key)
    ) {
      // Letter keybinds: run the matching item in the current view. Only active
      // at the root (empty query) or inside a submenu — otherwise it types into
      // the filter.
      if (activeSubmenu || query.trim() === "") {
        const item = viewKeybinds.get(e.key.toLowerCase());
        if (item) {
          e.preventDefault();
          openItem(item);
        }
      }
    }
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
</script>

<ModalShell
  open={commandPalette.open}
  ariaLabel="Command palette"
  onClose={close}
  onKeydown={onModalKeydown}
  closeOnEscape={false}
  focusSelector="input"
  backdropClass="fixed inset-0 z-[100] bg-bg/50"
  panelClass="fixed left-1/2 -translate-x-1/2 top-[12vh] w-[640px] z-[101] rounded-xl bg-card border border-border shadow-2xl overflow-hidden outline-none"
>
  <div class="flex items-center gap-3 px-4 py-3 border-b border-hairline">
    {#if activeSubmenu}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <button
        type="button"
        aria-label="Back"
        onclick={goBack}
        class="text-muted hover:text-fg-2"
      >←</button>
      <span class="text-sm text-fg-2 font-medium">{activeSubmenu.label}</span>
    {:else}
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-muted"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
    {/if}
    {#if activeSubmenu?.view !== "reviewers"}
      <input
        bind:this={inputEl}
        bind:value={query}
        class="flex-1 bg-transparent outline-none text-base placeholder:text-muted"
        placeholder={activeSubmenu ? "Filter…" : "Type a command or jump to file…"}
      />
      <span class="kbd">esc</span>
    {:else}
      <span class="ml-auto text-[10px] mono text-muted">{reviewerSelection.size} selected</span>
    {/if}
  </div>

  <div bind:this={listEl} class="max-h-[60vh] overflow-y-auto py-1">
    {#snippet paletteRow(item: CommandItem, idx: number, isActive: boolean)}
      <button
        data-active={isActive}
        onclick={() => openItem(item)}
        onmouseenter={() => (selectedIdx = idx)}
        class="w-full flex items-center gap-3 px-4 py-2 text-left transition-colors {isActive ? 'bg-hover' : 'hover:bg-hover'}"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class={isActive ? "text-accent" : "text-fg-3"}><circle cx="12" cy="12" r="9"/></svg>
        <div class="flex-1 min-w-0">
          <div class="text-sm {isActive ? 'text-fg' : 'text-fg-2'}">
            {#each highlight(item.label, query) as part}{#if part.match}<span class="text-accent font-medium">{part.match}</span>{:else}{part.rest}{/if}{/each}
          </div>
          {#if item.submenuOf}
            <div class="text-[11px] text-muted">in {item.submenuOf}</div>
          {:else if item.description}
            <div class="text-[11px] text-muted">{item.description}</div>
          {/if}
        </div>
        {#if item.kbd}
          <span class="kbd">{item.kbd}</span>
        {/if}
        {#if item.submenuItems || item.view}
          <span class="text-fg-3 text-xs">›</span>
        {/if}
      </button>
    {/snippet}
    {#if activeSubmenu?.view === "reviewers"}
      <div class="flex flex-col h-[40vh]">
        <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
          <ReviewerPickerList
            bind:this={reviewerPickerRef}
            selected={reviewerSelection}
            onSelectedChange={(s) => (reviewerSelection = s)}
            bind:highlightIdx={reviewerHighlight}
          />
        </div>
        <div class="px-4 py-3 border-t border-hairline flex items-center justify-end gap-2 shrink-0">
          <button
            type="button"
            onclick={goBack}
            class="px-3 py-1.5 rounded-md text-xs font-medium text-muted hover:text-fg"
          >Back</button>
          <button
            type="button"
            onclick={() => void runSelectedReviewers()}
            disabled={reviewerSelection.size === 0}
            class="px-3 py-1.5 rounded-md text-xs font-medium bg-comment text-on-accent disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Run {reviewerSelection.size} reviewer{reviewerSelection.size === 1 ? "" : "s"}
          </button>
        </div>
      </div>
    {:else if activeSubmenu}
      {#each filtered as item, _i (item.id)}
        {@render paletteRow(item, _i, _i === selectedIdx)}
      {/each}
      {#if filtered.length === 0}
        <div class="px-4 py-6 text-center text-sm text-muted">No matches</div>
      {/if}
    {:else if query.trim() !== ""}
      {#each searchResults as item, _i (item.id)}
        {@render paletteRow(item, _i, _i === selectedIdx)}
      {/each}
      {#if searchResults.length === 0}
        <div class="px-4 py-6 text-center text-sm text-muted">No matches</div>
      {/if}
    {:else}
      {#each flat as item, _i (item.id)}
        {@render paletteRow(item, _i, _i === selectedIdx)}
      {/each}
    {/if}
    {#if navList.length === 0 && !activeSubmenu && query.trim() === ""}
      <div class="px-4 py-6 text-center text-sm text-muted">No commands</div>
    {/if}
  </div>

  <div class="border-t border-hairline px-4 py-2 flex items-center gap-3 text-[11px] text-muted">
    <span class="flex items-center gap-1"><span class="kbd">↑</span><span class="kbd">↓</span> nav</span>
    <span class="flex items-center gap-1"><span class="kbd">→</span> open</span>
    <span class="flex items-center gap-1"><span class="kbd">←</span> back</span>
    <span class="flex items-center gap-1"><span class="kbd">⏎</span> run</span>
    <span class="ml-auto flex items-center gap-1">
      <span>letters</span><span>jump to item</span>
    </span>
  </div>
</ModalShell>
