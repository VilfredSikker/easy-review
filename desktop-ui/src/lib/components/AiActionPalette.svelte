<script lang="ts" module>
  let dismissPalette: (() => void) | null = null;

  /** Close the AI Hub palette from anywhere (e.g. when a review command starts). */
  export function closeAiActionPalette(): void {
    dismissPalette?.();
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { registerAiPaletteOpener } from "$lib/stores/keyboard";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import ReviewerPickerList from "$lib/components/ReviewerPickerList.svelte";
  import { openAiReviewFilesModal } from "$lib/components/AiReviewFilesModal.svelte";
  import { openProfessorFocusModal } from "$lib/components/ProfessorFocusModal.svelte";
  import { effortLabel, effortLevelsForModel, modelSupportsEffort } from "$lib/arena/effort";
  import { reviewScopeFromMode, scopeDescriptionFromMode } from "$lib/reviewScope";
  import type { AiProviderInfo } from "$lib/types";

  type SubView = "main" | "providers" | "models" | "reviewers";

  let open = $state(false);
  let selectedIdx = $state(0);
  let subView = $state<SubView>("main");
  let providers = $state<AiProviderInfo[]>([]);
  let selectedProvider = $state<AiProviderInfo | null>(null);
  let selectedReviewers = $state<Set<string>>(new Set());
  let reviewerHighlight = $state(0);
  let reviewerPickerRef = $state<{ moveHighlight: (d: number) => void; toggleHighlighted: () => void } | null>(null);

  interface AiAction {
    id: string;
    label: string;
    description: string;
    kbd?: string;
    disabled?: boolean;
    run: () => void;
  }

  function close() {
    open = false;
    selectedIdx = 0;
    subView = "main";
    providers = [];
    selectedProvider = null;
    selectedReviewers = new Set();
    reviewerHighlight = 0;
  }

  function dismissAndRun(fn: () => void) {
    close();
    queueMicrotask(fn);
  }

  function openPalette() {
    selectedIdx = 0;
    subView = "main";
    providers = [];
    selectedProvider = null;
    selectedReviewers = new Set();
    open = true;
  }

  function openPaletteFromOutside() {
    openPalette();
  }

  function goBack() {
    if (subView === "models") {
      subView = "providers";
      selectedIdx = providers.findIndex((p) => p.id === selectedProvider?.id);
      if (selectedIdx < 0) selectedIdx = 0;
    } else if (subView === "providers" || subView === "reviewers") {
      subView = "main";
      selectedIdx = 0;
    }
  }

  function openReviewerPicker() {
    if (!reviewScope) return;
    subView = "reviewers";
    reviewerHighlight = 0;
  }

  async function runSelectedReviewers() {
    if (!reviewScope || selectedReviewers.size === 0) return;
    const scope = reviewScope;
    const kinds = [...selectedReviewers];
    if (kinds.includes("professor")) {
      dismissAndRun(() => openProfessorFocusModal(scope, kinds, []));
      return;
    }
    dismissAndRun(() =>
      void app.cmd("run_ai_scoped_review", {
        scope,
        paths: [],
        reviewerKinds: kinds,
        focusPrompt: null,
      }),
    );
  }

  async function openProviderPicker() {
    try {
      providers = await invoke<AiProviderInfo[]>("list_ai_providers");
      if (providers.length === 0) {
        app.showToast("error", "No [ai_hub] providers configured — edit .er-config.toml to add providers");
        return;
      }
      subView = "providers";
      selectedIdx = Math.max(0, providers.findIndex((p) => p.is_selected));
    } catch (e) {
      app.showToast("error", `list_ai_providers: ${e}`);
    }
  }

  function selectProvider(provider: AiProviderInfo) {
    if (provider.models.length === 0) {
      app.cmd("set_ai_selection", { providerId: provider.id, modelId: null });
      close();
    } else {
      selectedProvider = provider;
      subView = "models";
      selectedIdx = Math.max(0, provider.models.findIndex((m) => m.is_selected));
    }
  }

  function selectModel(modelId: string) {
    if (!selectedProvider) return;
    app.cmd("set_ai_selection", { providerId: selectedProvider.id, modelId: modelId });
    if (selectedProvider.id !== "claude" || !modelSupportsEffort(modelId)) {
      close();
    }
  }

  function setEffort(level: string) {
    void app.cmd("set_ai_effort", { effort: level });
  }

  const activeAiLabel = $derived(app.snapshot?.active_ai_label ?? "");
  const activeEffort = $derived(app.snapshot?.active_ai_effort ?? null);
  const selectedModelId = $derived(
    selectedProvider?.models.find((m) => m.is_selected)?.id ??
      selectedProvider?.models[0]?.id ??
      "",
  );
  const effortLevels = $derived(
    selectedProvider?.id === "claude" && selectedModelId
      ? effortLevelsForModel(selectedModelId)
      : [],
  );
  const showEffortPicker = $derived(subView === "models" && effortLevels.length > 0);
  const reviewerCount = $derived(selectedReviewers.size);

  const mode = $derived(app.snapshot?.mode);
  const reviewScope = $derived(reviewScopeFromMode(mode));
  const scopeDescription = $derived(scopeDescriptionFromMode(mode));

  const runningCommands = $derived(
    (app.snapshot?.agent_commands ?? []).filter((c) => c.status === "running")
  );

  const hasReviewJson = $derived(app.snapshot?.ai?.has_review_json ?? false);
  const eligibleCommentCount = $derived(app.snapshot?.ai?.eligible_comment_count ?? 0);
  const validateAvailable = $derived(hasReviewJson || eligibleCommentCount > 0);

  const validateDescription = $derived.by(() => {
    if (!reviewScope) return "Not available in this view";
    if (!validateAvailable) return "Run General review or add GitHub comments first";
    if (hasReviewJson && eligibleCommentCount > 0) {
      return `Re-anchor review findings and ${eligibleCommentCount} GitHub comment${eligibleCommentCount === 1 ? "" : "s"}`;
    }
    if (eligibleCommentCount > 0) {
      return `Re-anchor ${eligibleCommentCount} unresolved GitHub comment${eligibleCommentCount === 1 ? "" : "s"}`;
    }
    return `Re-anchor AI review findings (${scopeDescription.toLowerCase()})`;
  });

  const actions = $derived<AiAction[]>([
    {
      id: "triage-current",
      label: "Triage branch",
      description: reviewScope
        ? "Fast scan — first impression and review routing (Haiku-class model)"
        : "Not available in this view",
      run: () => {
        if (!reviewScope) return;
        dismissAndRun(() => void app.cmd("run_ai_triage_review", { scope: reviewScope }));
      },
    },
    {
      id: "review-current",
      label: "Run review",
      description: reviewScope
        ? `General review only — risk, order, checklist, summary (${scopeDescription.toLowerCase()})`
        : "Not available in this view",
      run: () => {
        if (!reviewScope) return;
        dismissAndRun(() => void app.cmd("run_ai_review", { scope: reviewScope }));
      },
    },
    {
      id: "review-reviewers",
      label: "Run reviewers…",
      description: reviewScope
        ? "Multi-select General, experts, and Professor"
        : "Not available in this view",
      run: () => {
        if (!reviewScope) return;
        openReviewerPicker();
      },
    },
    {
      id: "professor",
      label: "Professor",
      description: reviewScope
        ? "Learn what this diff implements (not a code review)"
        : "Not available in this view",
      run: () => {
        if (!reviewScope) return;
        dismissAndRun(() => openProfessorFocusModal(reviewScope, ["professor"], []));
      },
    },
    {
      id: "validate-current",
      label: "Validate / re-anchor",
      description: validateDescription,
      run: () => {
        if (!reviewScope || !validateAvailable) return;
        dismissAndRun(() => void app.cmd("run_ai_validate", { scope: reviewScope }));
      },
    },
    {
      id: "review-select-files",
      label: "Review select files",
      description: reviewScope
        ? `Choose files and reviewers (${scopeDescription.toLowerCase()})`
        : "Not available in this view",
      run: () => {
        if (!reviewScope) return;
        dismissAndRun(() => openAiReviewFilesModal());
      },
    },
    {
      id: "open-output",
      label: "Open agent output",
      description: runningCommands.length > 0
        ? `${runningCommands.length} command(s) running — view live output`
        : "View the agent log from the last run",
      run: () => dismissAndRun(() => app.setMainView("agent-output")),
    },
    {
      id: "copy-context",
      label: "Copy review context",
      description: "Export current diff context to clipboard",
      run: () => dismissAndRun(() => void app.cmd("export_to_agent")),
    },
    {
      id: "change-model",
      label: "Change provider / model",
      description: activeAiLabel ? `Currently: ${activeAiLabel}` : "Select AI provider and model",
      run: () => { openProviderPicker(); },
    },
  ]);

  const currentItems = $derived.by((): { label: string; description: string; disabled?: boolean; onSelect: () => void }[] => {
    if (subView === "providers") {
      return providers.map((p) => ({
        label: p.label,
        description: p.models.length > 0
          ? `${p.models.length} model${p.models.length === 1 ? "" : "s"}${p.is_selected ? " · active" : ""}`
          : p.is_selected ? "active" : "no model presets",
        onSelect: () => selectProvider(p),
      }));
    }
    if (subView === "models" && selectedProvider) {
      return selectedProvider.models.map((m) => ({
        label: m.label,
        description: m.is_selected ? "currently selected" : "",
        onSelect: () => selectModel(m.id),
      }));
    }
    return actions.map((a) => ({
      label: a.label,
      description: a.description,
      disabled: a.disabled ?? (
        (!reviewScope && (a.id === "triage-current" || a.id === "review-current" || a.id === "validate-current" || a.id === "review-select-files" || a.id === "review-reviewers" || a.id === "professor"))
        || (a.id === "validate-current" && !validateAvailable)
      ),
      onSelect: a.run,
    }));
  });

  const subViewTitle = $derived(
    subView === "providers" ? "Select provider" :
    subView === "models" ? `${selectedProvider?.label ?? "Select model"}` :
    subView === "reviewers" ? "Choose reviewers" :
    "AI Actions"
  );

  const panelClass = $derived(
    subView === "reviewers"
      ? "fixed left-1/2 -translate-x-1/2 top-[12vh] z-[251] bg-ink-800 border border-ink-500 rounded-lg shadow-2xl w-[min(520px,calc(100vw-2rem))] h-[min(70vh,640px)] max-h-[calc(100vh-3rem)] flex flex-col overflow-hidden outline-none"
      : "fixed left-1/2 -translate-x-1/2 top-[15vh] z-[251] bg-ink-800 border border-ink-500 rounded-lg shadow-2xl w-[480px] max-w-[calc(100vw-2rem)] overflow-hidden outline-none"
  );

  function handleKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (subView === "reviewers") {
      if (e.key === "Escape") {
        e.preventDefault();
        goBack();
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        reviewerPickerRef?.moveHighlight(1);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        reviewerPickerRef?.moveHighlight(-1);
        return;
      }
      if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        if (e.key === "Enter" && reviewerCount > 0) void runSelectedReviewers();
        else reviewerPickerRef?.toggleHighlighted();
        return;
      }
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (subView !== "main") { goBack(); } else { close(); }
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIdx = (selectedIdx + 1) % currentItems.length;
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIdx = (selectedIdx - 1 + currentItems.length) % currentItems.length;
      return;
    }
    if (e.key === "ArrowLeft" && subView !== "main") {
      e.preventDefault();
      goBack();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const item = currentItems[selectedIdx];
      if (item && !item.disabled) item.onSelect();
      return;
    }
  }

  onMount(() => {
    dismissPalette = close;
    const cleanup = registerAiPaletteOpener(openPaletteFromOutside);
    return () => {
      dismissPalette = null;
      cleanup();
    };
  });
</script>

<ModalShell
  {open}
  ariaLabel={subViewTitle}
  onClose={close}
  onKeydown={handleKeydown}
  closeOnEscape={false}
  {panelClass}
>
  <div class="px-4 pt-3 pb-2 border-b border-ink-600 flex items-center gap-2 shrink-0">
    {#if subView !== "main"}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <span
        class="text-ink-400 hover:text-ink-200 cursor-pointer text-xs font-mono"
        onclick={goBack}
      >←</span>
    {:else}
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-accent shrink-0">
        <path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/>
      </svg>
    {/if}
    <span class="text-xs text-ink-300 font-mono">{subViewTitle}</span>
    {#if subView === "main"}
      {#if activeAiLabel}
        <span class="ml-auto text-[10px] text-ink-400 font-mono truncate max-w-[180px]">{activeAiLabel}</span>
      {/if}
      {#if runningCommands.length > 0}
        <span class="ml-1 w-1.5 h-1.5 rounded-full bg-accent animate-pulse shrink-0"></span>
      {/if}
    {/if}
    <kbd class="ml-auto shrink-0 text-[10px] font-mono px-1.5 py-0.5 rounded bg-ink-650 border border-ink-500 text-ink-400">Esc</kbd>
  </div>

  {#if subView === "reviewers"}
    <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
      <ReviewerPickerList
        bind:this={reviewerPickerRef}
        selected={selectedReviewers}
        onSelectedChange={(s) => (selectedReviewers = s)}
        bind:highlightIdx={reviewerHighlight}
      />
    </div>
    <div class="px-4 py-3 border-t border-ink-600 flex items-center justify-end gap-2 shrink-0">
      <Button variant="ghost" onclick={goBack}>Back</Button>
      <Button
        variant="primary"
        disabled={reviewerCount === 0}
        onclick={() => void runSelectedReviewers()}
      >
        Run {reviewerCount} reviewer{reviewerCount === 1 ? "" : "s"}
      </Button>
    </div>
  {:else}
    {#if showEffortPicker}
      <div class="px-4 py-2 border-b border-ink-600 shrink-0">
        <p class="text-[10px] uppercase tracking-wider text-ink-400 mb-1.5">Reasoning effort</p>
        <div class="flex flex-wrap gap-1">
          {#each effortLevels as level (level)}
            <button
              type="button"
              onclick={() => setEffort(level)}
              class="rounded px-2 py-1 text-[11px] font-medium transition-colors
                {activeEffort === level
                ? 'bg-accent text-on-accent'
                : 'bg-ink-700 text-ink-200 hover:bg-ink-650'}"
            >
              {effortLabel(level)}
            </button>
          {/each}
        </div>
        <p class="mt-1.5 text-[10px] text-ink-400">Applies to Claude reviews and arena runs.</p>
      </div>
    {/if}
    <div class="py-1">
      {#each currentItems as item, i}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="px-4 py-2.5 flex items-start gap-3 transition-colors {item.disabled ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'} {i === selectedIdx ? 'bg-ink-700' : item.disabled ? '' : 'hover:bg-ink-750'}"
          onclick={() => { if (!item.disabled) item.onSelect(); }}
          onmouseenter={() => { selectedIdx = i; }}
        >
          <div class="flex flex-col min-w-0">
            <span class="text-sm text-ink-100">{item.label}</span>
            {#if item.description}
              <span class="text-xs text-ink-300 mt-0.5">{item.description}</span>
            {/if}
          </div>
          {#if subView === "providers" && providers[i]?.models.length > 0}
            <span class="ml-auto shrink-0 text-ink-400 text-xs">›</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</ModalShell>
