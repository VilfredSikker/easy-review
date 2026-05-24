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
  import { openAiReviewFilesModal } from "$lib/components/AiReviewFilesModal.svelte";
  import type { AiProviderInfo, ExpertInfo } from "$lib/types";

  type SubView = "main" | "providers" | "models" | "experts";

  let open = $state(false);
  let selectedIdx = $state(0);
  let subView = $state<SubView>("main");
  let providers = $state<AiProviderInfo[]>([]);
  let experts = $state<ExpertInfo[]>([]);
  let selectedProvider = $state<AiProviderInfo | null>(null);

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
  }

  function dismissAndRun(fn: () => void) {
    close();
    queueMicrotask(fn);
  }

  function openPalette() {
    selectedIdx = 0;
    subView = "main";
    providers = [];
    experts = [];
    selectedProvider = null;
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
    } else if (subView === "providers" || subView === "experts") {
      subView = "main";
      selectedIdx = 0;
    }
  }

  async function openExpertPicker() {
    if (!reviewScope) return;
    try {
      experts = await invoke<ExpertInfo[]>("list_ai_experts");
      if (experts.length === 0) {
        app.showToast("error", "No expert reviewers configured");
        return;
      }
      subView = "experts";
      selectedIdx = 0;
    } catch (e) {
      app.showToast("error", `list_ai_experts: ${e}`);
    }
  }

  function runExpert(expert: ExpertInfo) {
    if (!reviewScope) return;
    dismissAndRun(() =>
      void app.cmd("run_ai_expert_review", { scope: reviewScope, expertId: expert.id }),
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
    close();
  }

  const activeAiLabel = $derived(app.snapshot?.active_ai_label ?? "");

  const mode = $derived(app.snapshot?.mode);
  const reviewScope = $derived(
    mode === "branch" || mode === "unstaged" || mode === "staged" ? mode : null,
  );
  const scopeDescription = $derived(
    mode === "branch"
      ? "All changes vs base"
      : mode === "unstaged"
        ? "Working tree changes"
        : mode === "staged"
          ? "Staged changes only"
          : "Switch to All changes, Unstaged, or Staged",
  );

  const runningCommands = $derived(
    (app.snapshot?.agent_commands ?? []).filter((c) => c.status === "running")
  );

  const actions = $derived<AiAction[]>([
    {
      id: "review-current",
      label: "Run review",
      description: reviewScope
        ? `Full review — risk, order, checklist, summary (${scopeDescription.toLowerCase()})`
        : "Not available in this view",
      run: () => {
        if (!reviewScope) return;
        dismissAndRun(() => void app.cmd("run_ai_review", { scope: reviewScope }));
      },
    },
    {
      id: "review-expert",
      label: "Run specialized review",
      description: reviewScope
        ? "Focused expert lens (security, patterns, …) — findings only"
        : "Not available in this view",
      run: () => {
        if (!reviewScope) return;
        void openExpertPicker();
      },
    },
    {
      id: "validate-current",
      label: "Validate / re-anchor review",
      description: reviewScope ? scopeDescription : "Not available in this view",
      run: () => {
        if (!reviewScope) return;
        dismissAndRun(() => void app.cmd("run_ai_validate", { scope: reviewScope }));
      },
    },
    {
      id: "review-select-files",
      label: "Review select files",
      description: reviewScope
        ? `Choose files to review (${scopeDescription.toLowerCase()})`
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
    if (subView === "experts") {
      return experts.map((e) => ({
        label: e.label,
        description: e.description,
        onSelect: () => runExpert(e),
      }));
    }
    return actions.map((a) => ({
      label: a.label,
      description: a.description,
      disabled: a.disabled ?? (!reviewScope && (a.id === "review-current" || a.id === "validate-current" || a.id === "review-select-files")),
      onSelect: a.run,
    }));
  });

  const subViewTitle = $derived(
    subView === "providers" ? "Select provider" :
    subView === "models" ? `${selectedProvider?.label ?? "Select model"}` :
    subView === "experts" ? "Specialized review" :
    "AI Actions"
  );

  function handleKeydown(e: KeyboardEvent) {
    if (!open) return;
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
  panelClass="fixed left-1/2 -translate-x-1/2 top-[15vh] z-[251] bg-ink-800 border border-ink-500 rounded-lg shadow-2xl w-[480px] max-w-[calc(100vw-2rem)] overflow-hidden outline-none"
>
      <!-- header -->
      <div class="px-4 pt-3 pb-2 border-b border-ink-600 flex items-center gap-2">
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

      <!-- items list -->
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
</ModalShell>
