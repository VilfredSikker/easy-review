<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { commandPalette } from "$lib/stores/commandPalette.svelte";
  import { rightRail } from "$lib/stores/rightRail.svelte";
  import { rightPanelTab } from "$lib/stores/rightPanelTab.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { reviewScopeFromMode, scopeDescriptionFromMode } from "$lib/reviewScope";
  import { openAiReviewFilesModal } from "$lib/components/AiReviewFilesModal.svelte";
  import { openProfessorFocusModal } from "$lib/components/ProfessorFocusModal.svelte";
  import ReviewerPickerList from "$lib/components/ReviewerPickerList.svelte";
  import type { AiModelInfo, AiProviderInfo, AppSnapshot } from "$lib/types";
  import type { ReviewScope } from "$lib/reviewScope";
  import {
    effortChoicesForModel,
    selectedModelDescription,
  } from "$lib/arena/effort";
  import { isPaletteSearchFocused, paletteQuickActionKey } from "$lib/commandPaletteKeys";
  import { keybindIndex, paletteKeyIntent, providerDescription, validateDescription, type PaletteKeyIntent } from "$lib/commandPaletteModel";
  import { AI_ADVANCED_GROUP, AI_GROUP_ORDER, AI_MENU_ACTION_ORDER, REVIEW_FLOW_ACTION_ID, reviewFlowControls, reviewFlowEstimate, reviewFlowStage } from "$lib/reviewFlow";
  import { actionsMenu, filesMenu, navigateMenu, prMenu, viewMenu, type CommandItem, type PaletteRunners } from "$lib/commandPaletteMenus";
  import { RUN_FLASH_MS, fileDir, groupByMenu, isRowKey, isSingleKey, paletteFooterContext, paletteGlyph, palettePlaceholder } from "$lib/commandPaletteView";
  import { resolveContextTitle } from "$lib/contextIdentity";
  import { baseName } from "$lib/shortFileName";

  let query = $state("");
  let selectedIdx = $state(0);
  let inputEl = $state<HTMLInputElement | null>(null);
  let listEl = $state<HTMLDivElement | null>(null);

  /** Stack of submenu entries — the top entry is the currently-open submenu.
   *  Each entry remembers the selected index + query of the view it was opened
   *  from, so ← returns to the same active item. */
  let submenuStack = $state<{ item: CommandItem; selectedIdx: number; query: string }[]>([]);

  // ── AI submenu state ─────────────────────────────────────────────────────
  let aiProviders = $state<AiProviderInfo[]>([]);
  let reviewerSelection = $state<Set<string>>(new Set());
  let reviewerHighlight = $state(0);
  let reviewerPickerRef = $state<{ moveHighlight: (d: number) => void; toggleHighlighted: () => void } | null>(null);

  /** The row showing the run flash; its action fires when the flash ends. */
  let flashId = $state<string | null>(null);
  let flashTimer: ReturnType<typeof setTimeout> | null = null;

  const snapshot = $derived(app.snapshot);

  function cancelFlash() {
    if (flashTimer !== null) clearTimeout(flashTimer);
    flashTimer = null;
    flashId = null;
  }

  /** Also clears the submenu stack, so the next open lands at the root. */
  function close() {
    cancelFlash();
    commandPalette.close();
    query = "";
    selectedIdx = 0;
    submenuStack = [];
    reviewerSelection = new Set();
  }

  /** Close the palette, let it paint, then run a heavy (IPC / snapshot) action. */
  async function dismissAndRun(fn: () => unknown) {
    close();
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

  const runners: PaletteRunners = {
    dismissAndRun: (fn) => { void dismissAndRun(fn); },
    dismissLocal,
  };

  // ── AI actions ───────────────────────────────────────────────────────────
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
    const validateHint = validateDescription({
      inScope: !!reviewScope,
      scopeDescription,
      hasReviewJson,
      eligibleCommentCount,
    });

    const guard = (fn: () => void) => () => { if (!reviewScope) return; fn(); };
    const scoped = (description: string) => (reviewScope ? description : "Not available in this view");

    const items: CommandItem[] = [
      {
        id: "ai-triage",
        label: "Triage branch",
        description: scoped("Fast scan — first impression and review routing (default model, low effort)"),
        group: "AI" as const,
        kbd: "t",
        run: guard(() => { void dismissAndRun(() => app.cmd("run_ai_triage_review", { scope: reviewScope })); }),
      },
      {
        id: "ai-run-review",
        label: "Run review",
        description: scoped(`General review only — risk, order, checklist, summary (${scopeDescription.toLowerCase()})`),
        group: "AI" as const,
        kbd: "r",
        run: guard(() => { void dismissAndRun(() => app.cmd("run_ai_review", { scope: reviewScope })); }),
      },
      {
        id: "ai-run-reviewers",
        label: "Run reviewers…",
        description: scoped("Multi-select General, experts, and Professor"),
        group: "AI" as const,
        kbd: "v",
        view: "reviewers" as const,
        run: () => {},
      },
      {
        id: "ai-professor",
        label: "Professor",
        description: scoped("Learn what this diff implements (not a code review)"),
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
        description: scoped("Group the diff into a guided walkthrough (pillars) for the Tour tab"),
        group: "AI" as const,
        kbd: "g",
        run: guard(() => { void dismissAndRun(() => app.cmd("generate_tour")); }),
      },
      {
        id: "ai-diagrams",
        label: "Diagrams",
        description: scoped("Generate mermaid diagrams of this diff (mental model, subsystems, flows)"),
        group: "AI" as const,
        kbd: "d",
        run: guard(() => {
          close();
          rightRail.expand();
          rightPanelTab.set("context");
        }),
      },
      {
        id: "ai-validate",
        label: "Validate / re-anchor",
        description: validateHint,
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
        description: scoped(`Choose files and reviewers (${scopeDescription.toLowerCase()})`),
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
    return buildReviewFlowItems(items, reviewScope, snapshot);
  }

  function buildReviewFlowItems(
    items: CommandItem[],
    reviewScope: ReviewScope | null,
    flowSnapshot: AppSnapshot | null,
  ): CommandItem[] {
    const flow = flowSnapshot?.review_flow;
    const controls = reviewFlowControls(flow);
    const flowAction = reviewFlowAction(flow, controls, reviewScope, flowSnapshot);
    const advancedIds = new Set(["ai-run-review", "ai-run-reviewers", "ai-validate"]);
    const advancedItems: CommandItem[] = items
      .filter((item) => advancedIds.has(item.id))
      .map((item) => ({ ...item, group: AI_GROUP_ORDER[1] }));
    advancedItems.push(arenaAdvancedItem());

    const visibleItems = items.filter((item) => !advancedIds.has(item.id));
    const triageItem = visibleItems.find((item) => item.id === AI_MENU_ACTION_ORDER[1]);
    const result: CommandItem[] = [flowAction];
    if (controls.decline) result.push(declineReviewFlowItem(flowSnapshot));
    if (triageItem) result.push(triageItem);
    result.push(advancedMenuItem(advancedItems));
    result.push(...visibleItems.filter((item) => item.id !== AI_MENU_ACTION_ORDER[1]));
    return result;
  }

  function reviewFlowAction(
    flow: AppSnapshot["review_flow"],
    controls: ReturnType<typeof reviewFlowControls>,
    reviewScope: ReviewScope | null,
    flowSnapshot: AppSnapshot | null,
  ): CommandItem {
    let label = "Review";
    if (controls.confirm) label = "Confirm Deep review";
    else if (controls.busy) label = flow ? reviewFlowStage(flow) : "Review running";

    let description = "Triage, review, and verify this diff";
    if (flow?.state === "awaiting_confirmation") {
      description = `${reviewFlowEstimate(flow)} · confirm Deep review`;
    }

    return {
      id: REVIEW_FLOW_ACTION_ID,
      label,
      description: reviewScope ? description : "Not available in this view",
      group: "AI",
      kbd: "r",
      run: () => {
        if (!reviewScope) return;
        const erDir = flowSnapshot?.review_bucket_er_dir;
        if (!erDir || controls.busy && !controls.confirm) return;
        const command = controls.confirm ? "confirm_review_flow" : "start_review_flow";
        const args = controls.confirm ? { erDir } : { scope: reviewScope, erDir };
        void dismissAndRun(() => app.cmd(command, args));
      },
    };
  }

  function declineReviewFlowItem(flowSnapshot: AppSnapshot | null): CommandItem {
    return {
      id: "ai-review-flow-decline",
      label: "Decline Deep review",
      description: "Decline the estimate and release the waiting review flow",
      group: AI_GROUP_ORDER[0],
      run: () => {
        const erDir = flowSnapshot?.review_bucket_er_dir;
        if (erDir) void dismissAndRun(() => app.cmd("decline_review_flow", { erDir }));
      },
    };
  }

  function arenaAdvancedItem(): CommandItem {
    return {
      id: "open-arena",
      label: "Open AI review arena",
      description: "Compare multiple reviewers on the current diff",
      group: AI_ADVANCED_GROUP,
      run: () => { dismissLocal(() => arena.openLauncher()); },
    };
  }

  function advancedMenuItem(items: CommandItem[]): CommandItem {
    return {
      id: AI_MENU_ACTION_ORDER[2],
      label: AI_ADVANCED_GROUP,
      description: "Manual review, reviewers, and validation",
      group: AI_GROUP_ORDER[0],
      run: () => {},
      submenuItems: items,
    };
  }

  // ── Provider / model / effort nested picker ──────────────────────────────
  function providerItems(): CommandItem[] {
    const currentEffort = snapshot?.active_ai_effort ?? null;
    return aiProviders.map((p) => ({
      id: `provider-${p.id}`,
      label: p.label,
      description: providerDescription(p),
      group: "AI" as const,
      run: () => {
        if (p.models.length === 0) {
          void dismissAndRun(() =>
            app.cmd("set_ai_selection", { providerId: p.id, modelId: null, persist: true }),
          );
        }
      },
      submenuItems: p.models.length > 0
        ? p.models.map((m) => modelItem(p.id, m, currentEffort))
        : undefined,
    }));
  }

  function modelItem(
    providerId: string,
    m: AiModelInfo,
    currentEffort: string | null,
  ): CommandItem {
    const choices = effortChoicesForModel(m, currentEffort);
    return {
      id: `model-${providerId}-${m.id}`,
      label: m.label,
      description: selectedModelDescription(m, currentEffort),
      group: "AI" as const,
      run: () => {
        if (choices.length === 0) {
          void dismissAndRun(() =>
            app.cmd("set_ai_selection", { providerId, modelId: m.id, persist: true }),
          );
        }
      },
      submenuItems: choices.length > 0
        ? choices.map((choice) => ({
            id: `effort-${providerId}-${m.id}-${choice.id}`,
            label: choice.label,
            description: choice.selected ? "currently selected" : "",
            group: "AI" as const,
            run: () => {
              void dismissAndRun(() =>
                app.cmd("set_ai_selection", {
                  providerId,
                  modelId: m.id,
                  effort: choice.id,
                  persist: true,
                }),
              );
            },
          }))
        : undefined,
    };
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
    const reviewScope = reviewScopeFromMode(snapshot?.mode);
    const activeAiLabel = snapshot?.active_ai_label ?? "";
    const aiDescription = reviewScope
      ? "Reviews, triage, Professor, model"
      : "Model picker available; reviews need a diff view";
    const fileItems = filesMenu(snapshot, runners);

    const containers: CommandItem[] = [
      { id: "menu-actions", label: "Actions", description: "Export, settings, logs", group: "Actions", kbd: "s", run: () => {}, submenuItems: actionsMenu(snapshot, runners) },
      { id: "menu-navigate", label: "Navigate", description: "Refresh, terminal, agent output", group: "Navigate", kbd: "n", run: () => {}, submenuItems: navigateMenu(snapshot, runners) },
      { id: "menu-view", label: "View & Layout", description: "Diff mode, browser, panels", group: "View & Layout", kbd: "v", run: () => {}, submenuItems: viewMenu(snapshot, runners) },
      // The active provider/model rides on the AI row: the top bar no longer shows it.
      { id: "menu-ai", label: "AI", description: activeAiLabel ? `${aiDescription} · ${activeAiLabel}` : aiDescription, group: "AI", kbd: "a", run: () => {}, submenuItems: buildAiItems() },
      { id: "menu-pr", label: "PR", description: "Open by URL, arena", group: "PR", kbd: "p", run: () => {}, submenuItems: prMenu(snapshot, runners) },
      ...(fileItems.length > 0
        ? [{ id: "menu-files", label: "Files in this diff", description: `${fileItems.length} files`, group: "Files in this diff" as const, kbd: "f", run: () => {}, submenuItems: fileItems }]
        : []),
    ];

    return containers;
  }

  /** Every item below the root menus, for search. Nested menus (AI · Advanced)
   *  are listed themselves and through their items. */
  function flattenLeaves(items: CommandItem[]): CommandItem[] {
    const out: CommandItem[] = [];
    for (const item of items) {
      out.push(item);
      if (item.submenuItems) out.push(...flattenLeaves(item.submenuItems));
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

  /** Push a submenu (a parent entry) onto the stack and reset nav.
   *  Records the current view's selected index + query so ← restores them.
   *  Search stays focused only if it already was — letter jumps do not steal
   *  the filter, and opening a menu does not auto-focus the field. */
  function pushSubmenu(parent: CommandItem) {
    const keepSearchFocus = document.activeElement === inputEl;
    submenuStack = [...submenuStack, { item: parent, selectedIdx, query }];
    query = "";
    selectedIdx = 0;
    if (parent.view === "reviewers") reviewerSelection = new Set();
    if (keepSearchFocus) queueMicrotask(() => inputEl?.focus());
  }

  function prefersReducedMotion(): boolean {
    return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  /** Flash the row, then run it. The flash confirms which row a letter hit
   *  before the palette disappears; closing the palette meanwhile cancels it. */
  function runWithFlash(item: CommandItem) {
    if (flashTimer !== null) return;
    if (prefersReducedMotion()) {
      item.run();
      return;
    }
    flashId = item.id;
    flashTimer = setTimeout(() => {
      flashTimer = null;
      flashId = null;
      item.run();
    }, RUN_FLASH_MS);
  }

  /** Open a list item: push a submenu if it has one, otherwise run it. */
  function openItem(item: CommandItem) {
    if (item.submenuItems || item.view) pushSubmenu(item);
    else runWithFlash(item);
  }

  /** Pop the current submenu (Esc or ←). Restores the position (selected item
   *  + filter) of the view we came from, at any nesting level. */
  function goBack() {
    const prev = submenuStack[submenuStack.length - 1];
    submenuStack = submenuStack.slice(0, -1);
    query = prev?.query ?? "";
    selectedIdx = prev?.selectedIdx ?? 0;
    const keepSearchFocus = document.activeElement === inputEl;
    if (keepSearchFocus) queueMicrotask(() => inputEl?.focus());
  }

  /** The top item of the submenu stack, or null when showing the root list. */
  const activeSubmenu = $derived(submenuStack[submenuStack.length - 1]?.item ?? null);

  /** Root items: the menu containers (Actions / Navigate / … / Files).
   *  Only build while open — when closed, `$effect`s below still subscribe to
   *  `navList`, and rebuilding the full file list on every poll snapshot was
   *  free work that amplified ⌘K action jank. */
  const allRootItems = $derived(commandPalette.open ? buildItems() : []);

  const searchableItems = $derived(
    commandPalette.open ? flattenLeaves(allRootItems.flatMap((menu) => menu.submenuItems ?? [])) : [],
  );

  /** Root search: every menu's matches, each menu's together under its name. */
  const searchRows = $derived(
    query.trim() === ""
      ? []
      : groupByMenu(searchableItems.filter((item) => matches(item.label, query))),
  );

  /** Items shown in a submenu (filtered by the query). */
  const filtered = $derived(
    activeSubmenu
      ? (activeSubmenu.submenuItems ?? []).filter((item) => matches(item.label, query))
      : [],
  );

  /** The list navigated by arrows/Enter in the current view, in render order. */
  const navList = $derived.by(() => {
    if (activeSubmenu) return filtered;
    return query.trim() !== "" ? searchRows.map((row) => row.item) : allRootItems;
  });

  /** Group header to draw above each row; only root search has them. */
  const rowHeaders = $derived(
    !activeSubmenu && query.trim() !== "" ? searchRows.map((row) => row.header) : [],
  );

  const crumb = $derived(submenuStack.map((entry) => entry.item.label).join(" › "));
  const footerContext = $derived(paletteFooterContext(resolveContextTitle(snapshot)));

  /** Single-letter keybinds for the current view's items (root containers or
   *  the active submenu's actions), e.g. a → AI, t → Triage, r → Run review. */
  const viewKeybinds = $derived(keybindIndex(navList));

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

  function rowTint(active: boolean, flashing: boolean): string {
    if (flashing) return "bg-[color-mix(in_srgb,var(--color-accent)_28%,var(--color-hover))]";
    return active ? "bg-[color-mix(in_srgb,var(--color-accent)_12%,var(--color-hover))]" : "";
  }

  /** A single key runs the row, so it is filled; a chord only documents a global shortcut. */
  function keyTone(single: boolean, active: boolean): string {
    if (!single) return "text-muted border-border";
    if (active) return "bg-hover font-medium text-accent border-[color-mix(in_srgb,var(--color-accent)_60%,transparent)]";
    return "bg-hover font-medium text-fg border-border";
  }

  function openPalette() {
    selectedIdx = 0;
    query = "";
    submenuStack = [];
    reviewerSelection = new Set();
    commandPalette.show();
  }

  function focusSearch() {
    inputEl?.focus();
  }

  const KEY_INTENT_HANDLERS: Record<Exclude<PaletteKeyIntent, "none">, (e: KeyboardEvent) => void> = {
    swallow: () => {},
    back: () => goBack(),
    leave: () => {
      if (activeSubmenu) goBack();
      else close();
    },
    "clear-query": () => {
      query = "";
      selectedIdx = 0;
    },
    "blur-search": () => inputEl?.blur(),
    "open-selected-submenu": () => {
      const item = navList[selectedIdx];
      if (item && (item.submenuItems || item.view)) openItem(item);
    },
    "open-selected": () => {
      const item = navList[selectedIdx];
      if (item) openItem(item);
    },
    next: () => { selectedIdx = Math.min(selectedIdx + 1, navList.length - 1); },
    prev: () => { selectedIdx = Math.max(selectedIdx - 1, 0); },
    "focus-search": () => focusSearch(),
    letter: (e) => {
      const item = viewKeybinds.get(e.key.toLowerCase());
      if (item) openItem(item);
    },
    "reviewer-down": () => reviewerPickerRef?.moveHighlight(1),
    "reviewer-up": () => reviewerPickerRef?.moveHighlight(-1),
    "reviewer-toggle": () => reviewerPickerRef?.toggleHighlighted(),
    "reviewer-run": () => {
      if (reviewerSelection.size > 0) void runSelectedReviewers();
    },
  };

  function onModalKeydown(e: KeyboardEvent) {
    if (!commandPalette.open) return;
    const searchFocused = isPaletteSearchFocused(e.target);
    const intent = paletteKeyIntent(e, {
      reviewersView: activeSubmenu?.view === "reviewers",
      searchFocused,
      hasQuery: query.trim() !== "",
      inSubmenu: activeSubmenu != null,
      quickAction: paletteQuickActionKey(e, searchFocused)
        ?? (isRowKey(e, searchFocused, (k) => viewKeybinds.has(k)) ? "letter" : null),
    });
    if (intent === "none") return;
    e.preventDefault();
    KEY_INTENT_HANDLERS[intent](e);
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

  $effect(() => {
    if (!commandPalette.open) return;
    const view = commandPalette.pendingView;
    if (view === "root") return;
    untrack(() => {
      commandPalette.consumePendingView();
      submenuStack = [];
      query = "";
      selectedIdx = 0;
      if (view === "ai-providers") {
        void openProviderPicker();
        return;
      }
      const menu = allRootItems.find((item) => item.id === view);
      if (menu) pushSubmenu(menu);
    });
  });

  // The store can be closed from outside (another modal, a view switch); a
  // pending run must not fire into whatever comes next.
  $effect(() => {
    if (!commandPalette.open) untrack(cancelFlash);
  });
</script>

{#snippet kbd(text: string)}
  <kbd class="font-mono text-[10px] px-1 mr-0.5 border border-border rounded-[3px] text-fg-2">{text}</kbd>
{/snippet}

{#snippet rowLabel(item: CommandItem)}
  {@const text = item.file ? baseName(item.file.path) : item.label}
  {@const description = item.file ? fileDir(item.file.path) : item.description}
  <span class="min-w-0 truncate text-fg">
    {#each highlight(text, query) as part, i (i)}{#if part.match}<span class="text-accent font-medium">{part.match}</span>{:else}{part.rest}{/if}{/each}
    {#if description}<span class="ml-2 text-xs text-muted">{description}</span>{/if}
  </span>
{/snippet}

{#snippet paletteRow(item: CommandItem, idx: number, isActive: boolean)}
  {@const flashing = flashId === item.id}
  <button type="button"
    role="option"
    aria-selected={isActive}
    data-active={isActive}
    title={item.file?.path}
    onclick={() => openItem(item)}
    onmouseenter={() => (selectedIdx = idx)}
    class="w-full grid grid-cols-[18px_minmax(0,1fr)_auto] gap-2.5 items-center px-2.5 py-[7px] rounded-[7px] text-left text-[13px] {rowTint(isActive, flashing)}"
  >
    <span class="font-mono text-[11px] text-center {isActive ? 'text-accent' : 'text-muted'}">{paletteGlyph(item.group)}</span>
    {@render rowLabel(item)}
    <span class="inline-flex items-center gap-2">
      {#if item.kbd}
        <kbd class="font-mono text-[10.5px] leading-[17px] min-w-[19px] px-[5px] rounded border text-center {keyTone(isSingleKey(item.kbd), isActive)}">{item.kbd}</kbd>
      {/if}
      {#if item.submenuItems || item.view}
        <span class="text-muted">›</span>
      {/if}
    </span>
  </button>
{/snippet}

<!-- Prototype .palette: --bg1 → card, --bg2 → hover, --line → line, --line2 → border, --tx3 → muted (ADR 0030). -->
<ModalShell
  open={commandPalette.open}
  ariaLabel="Command palette"
  onClose={close}
  onKeydown={onModalKeydown}
  closeOnEscape={false}
  backdropClass="fixed inset-0 z-[100] bg-[color-mix(in_srgb,var(--color-ground)_55%,transparent)]"
  panelClass="fixed left-1/2 -translate-x-1/2 top-[72px] w-[calc(100%-32px)] max-w-[620px] max-h-[calc(100vh-88px)] z-[101] flex flex-col rounded-xl bg-card border border-border shadow-2xl overflow-hidden outline-none"
>

  <div class="flex items-center gap-2.5 px-3.5 py-3 border-b border-line text-muted">
    {#if activeSubmenu}
      <button
        type="button"
        aria-label="Back"
        onclick={goBack}
        class="w-[22px] h-[22px] shrink-0 rounded-[5px] text-base leading-none text-fg-2 hover:bg-hover hover:text-fg"
      >‹</button>
      <span class="text-xs font-semibold text-fg px-2 py-0.5 rounded-[5px] bg-hover whitespace-nowrap" data-testid="palette-crumb">{crumb}</span>
    {/if}
    <button
      type="button"
      aria-label="Focus search"
      onclick={focusSearch}
      class="shrink-0 hover:text-fg-2"
    >
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5L14 14"/></svg>
    </button>
    {#if activeSubmenu?.view !== "reviewers"}
      <input
        bind:this={inputEl}
        bind:value={query}
        data-palette-search="true"
        aria-label="Search commands"
        autocomplete="off"
        class="flex-1 min-w-0 bg-transparent outline-none text-[15px] text-fg placeholder:text-muted"
        placeholder={palettePlaceholder(activeSubmenu?.label ?? null)}
      />
      {@render kbd("esc")}
    {:else}
      <span class="ml-auto text-[10px] mono text-muted">{reviewerSelection.size} selected</span>
    {/if}
  </div>


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
      <div class="px-4 py-3 border-t border-line flex items-center justify-end gap-2 shrink-0">
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
  {:else}
    <div bind:this={listEl} role="listbox" aria-label="Commands" class="flex-1 min-h-0 overflow-y-auto p-1.5">
      {#each navList as item, idx (item.id)}
        {#if rowHeaders[idx]}
          <div class="px-2.5 pt-2.5 pb-1 text-[10.5px] font-semibold tracking-[.06em] uppercase text-muted">{rowHeaders[idx]}</div>
        {/if}
        {@render paletteRow(item, idx, idx === selectedIdx)}
      {/each}
      {#if navList.length === 0}
        <div class="p-4 text-[12.5px] text-muted">No commands or files match.</div>
      {/if}
    </div>
  {/if}

  <div class="flex flex-wrap items-center gap-3.5 px-3.5 py-2 border-t border-line text-[11px] text-muted">
    <span>{@render kbd("a")}{@render kbd("r")} letters run the matching item</span>
    <span>{@render kbd("/")} search</span>
    <span>{@render kbd("←")} back</span>
    <span>{@render kbd("↵")} run</span>
    <span class="flex-1"></span>
    <span data-testid="palette-context">{footerContext}</span>
  </div>
</ModalShell>
