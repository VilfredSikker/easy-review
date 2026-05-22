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
  import type { AiProviderInfo } from "$lib/types";

  type SubView = "main" | "providers" | "models";

  let open = $state(false);
  let selectedIdx = $state(0);
  let subView = $state<SubView>("main");
  let providers = $state<AiProviderInfo[]>([]);
  let selectedProvider = $state<AiProviderInfo | null>(null);

  interface AiAction {
    id: string;
    label: string;
    description: string;
    kbd?: string;
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
    } else if (subView === "providers") {
      subView = "main";
      selectedIdx = 0;
    }
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

  const runningCommands = $derived(
    (app.snapshot?.agent_commands ?? []).filter((c) => c.status === "running")
  );

  const actions = $derived<AiAction[]>([
    {
      id: "review-branch",
      label: "Run review: branch",
      description: "Review all changes on this branch vs base",
      run: () => dismissAndRun(() => void app.cmd("run_ai_review", { scope: "branch" })),
    },
    {
      id: "review-unstaged",
      label: "Run review: unstaged",
      description: "Review only unstaged (working tree) changes",
      run: () => dismissAndRun(() => void app.cmd("run_ai_review", { scope: "unstaged" })),
    },
    {
      id: "review-staged",
      label: "Run review: staged",
      description: "Review only staged changes",
      run: () => dismissAndRun(() => void app.cmd("run_ai_review", { scope: "staged" })),
    },
    {
      id: "validate-branch",
      label: "Validate / re-anchor review: branch",
      description: "Re-check existing findings and re-anchor moved code",
      run: () => dismissAndRun(() => void app.cmd("run_ai_validate", { scope: "branch" })),
    },
    {
      id: "validate-unstaged",
      label: "Validate / re-anchor review: unstaged",
      description: "Validate findings against working tree changes",
      run: () => dismissAndRun(() => void app.cmd("run_ai_validate", { scope: "unstaged" })),
    },
    {
      id: "validate-staged",
      label: "Validate / re-anchor review: staged",
      description: "Validate findings against staged changes",
      run: () => dismissAndRun(() => void app.cmd("run_ai_validate", { scope: "staged" })),
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

  const currentItems = $derived.by((): { label: string; description: string; onSelect: () => void }[] => {
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
    return actions.map((a) => ({ label: a.label, description: a.description, onSelect: a.run }));
  });

  const subViewTitle = $derived(
    subView === "providers" ? "Select provider" :
    subView === "models" ? `${selectedProvider?.label ?? "Select model"}` :
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
      currentItems[selectedIdx]?.onSelect();
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
            class="px-4 py-2.5 flex items-start gap-3 cursor-pointer transition-colors {i === selectedIdx ? 'bg-ink-700' : 'hover:bg-ink-750'}"
            onclick={item.onSelect}
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
