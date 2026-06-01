<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import type {
    AiProviderInfo,
    ConfigFieldValue,
    ConfigHubField,
    GetConfigHubResponse,
    SettingsTab,
  } from "$lib/types";
  import Toggle from "./Toggle.svelte";
  import OptionGroup from "./OptionGroup.svelte";
  import SettingsTextField from "./SettingsTextField.svelte";
  import Button from "$lib/components/ui/Button.svelte";

  interface Props {
    onBack: () => void;
  }

  const { onBack }: Props = $props();

  const TABS: { id: SettingsTab; label: string; blurb: string }[] = [
    { id: "general", label: "General", blurb: "Shared config for both app and terminal." },
    { id: "app", label: "App", blurb: "Desktop app only." },
    { id: "terminal", label: "Terminal", blurb: "TUI (`er`) only." },
  ];

  let loading = $state(true);
  let activeTab = $state<SettingsTab>("general");
  let generalFields = $state<ConfigHubField[]>([]);
  let appFields = $state<ConfigHubField[]>([]);
  let terminalFields = $state<ConfigHubField[]>([]);
  let providers = $state<AiProviderInfo[]>([]);
  let hasLocalConfig = $state(false);
  let repoRoot = $state("");
  let addPattern = $state("");
  let textWarnings = $state<Record<string, string | null>>({});

  const fields = $derived(
    activeTab === "general"
      ? generalFields
      : activeTab === "app"
        ? appFields
        : terminalFields,
  );

  const tabBlurb = $derived(TABS.find((t) => t.id === activeTab)?.blurb ?? "");

  const selectedProvider = $derived(providers.find((p) => p.is_selected) ?? null);
  const selectedModels = $derived(selectedProvider?.models ?? []);

  function applySettings(res: GetConfigHubResponse) {
    generalFields = res.settings.general;
    appFields = res.settings.app;
    terminalFields = res.settings.terminal;
    providers = res.providers;
    hasLocalConfig = res.settings.hasLocalConfig;
    repoRoot = res.settings.repoRoot;
  }

  async function reload(resetBaseline = false) {
    loading = true;
    try {
      const res = await invoke<GetConfigHubResponse>("get_config_hub", {
        resetBaseline,
      });
      applySettings(res);
    } catch (e) {
      app.showToast("error", `get_config_hub: ${e}`);
    } finally {
      loading = false;
    }
  }

  async function patch(key: string, value: ConfigFieldValue) {
    try {
      const res = await invoke<GetConfigHubResponse>("apply_config_patch", {
        patch: { key, value },
      });
      applySettings(res);
    } catch (e) {
      app.showToast("error", `apply_config_patch: ${e}`);
    }
  }

  async function selectProvider(providerId: string) {
    const p = providers.find((x) => x.id === providerId);
    if (!p) return;
    if (p.models.length === 0) {
      await invoke("set_ai_selection", { providerId, modelId: null });
      await reload(false);
      return;
    }
    const model = p.models.find((m) => m.is_selected) ?? p.models[0];
    await invoke("set_ai_selection", { providerId, modelId: model?.id ?? null });
    await reload(false);
  }

  async function selectModel(modelId: string) {
    const p = selectedProvider;
    if (!p) return;
    await invoke("set_ai_selection", { providerId: p.id, modelId });
    await reload(false);
  }

  async function saveDefaults() {
    const p = selectedProvider;
    if (!p) return;
    const model = p.models.find((m) => m.is_selected);
    try {
      const res = await invoke<GetConfigHubResponse>("set_ai_hub_defaults", {
        providerId: p.id,
        modelId: model?.id ?? null,
      });
      applySettings(res);
      app.showToast("success", "Saved AI defaults to config");
    } catch (e) {
      app.showToast("error", `set_ai_hub_defaults: ${e}`);
    }
  }

  async function revert() {
    try {
      const res = await invoke<GetConfigHubResponse>("reset_config_draft");
      applySettings(res);
      app.showToast("info", "Reverted unsaved changes");
    } catch (e) {
      app.showToast("error", `reset_config_draft: ${e}`);
    }
  }

  async function saveLocal() {
    try {
      await invoke("save_config_local_cmd");
      hasLocalConfig = true;
      app.showToast("success", "Saved to .er-config.toml");
    } catch (e) {
      app.showToast("error", `save_config_local_cmd: ${e}`);
    }
  }

  async function saveGlobal() {
    try {
      await invoke("save_config_global_cmd");
      app.showToast("success", "Saved to global config");
    } catch (e) {
      app.showToast("error", `save_config_global_cmd: ${e}`);
    }
  }

  function validateText(key: string, value: string) {
    if (key === "agent.args" && !value.includes("{prompt}")) {
      textWarnings[key] = "Include {prompt} in args so the agent receives user input.";
    } else if (key === "agent.command" && !value.trim()) {
      textWarnings[key] = "Command cannot be empty.";
    } else {
      textWarnings[key] = null;
    }
  }

  $effect(() => {
    void reload(true);
  });
</script>

<div class="flex flex-col flex-1 w-full min-w-0 h-full min-h-0 bg-bg text-fg">
  <header class="shrink-0 flex items-center gap-3 px-4 py-3 border-b border-hairline">
    <button
      type="button"
      class="text-sm text-fg-3 hover:text-fg"
      onclick={onBack}
    >
      ← Back
    </button>
    <h1 class="text-base font-semibold">Settings</h1>
    <span class="text-xs text-muted truncate ml-auto font-mono" title={repoRoot}>
      {hasLocalConfig ? ".er-config.toml" : "global defaults"}
    </span>
  </header>

  {#if loading}
    <div class="flex-1 flex items-center justify-center text-sm text-muted">Loading…</div>
  {:else}
    <div
      class="shrink-0 flex gap-1 px-4 pt-3 border-b border-hairline"
      role="tablist"
      aria-label="Settings sections"
    >
      {#each TABS as tab (tab.id)}
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === tab.id}
          class="px-3 py-1.5 text-sm rounded-t-md border-b-2 transition-colors {activeTab === tab.id
            ? 'border-accent text-fg font-medium'
            : 'border-transparent text-fg-3 hover:text-fg hover:bg-hover'}"
          onclick={() => (activeTab = tab.id)}
        >
          {tab.label}
        </button>
      {/each}
    </div>

    <div class="flex-1 overflow-y-auto w-full px-6 py-4">
      <p class="text-xs text-muted mb-6">{tabBlurb}</p>

      {#each fields as field, i (field.kind === "section" ? field.title : "field-" + i + (field.kind === "bool" || field.kind === "cycle" || field.kind === "text" ? field.key : field.kind === "listEntry" ? field.key : field.label))}
        {#if field.kind === "section"}
          <h2 class="text-xs uppercase tracking-wider text-muted font-semibold mt-6 mb-2 first:mt-0">
            {field.title}
          </h2>
        {:else if field.kind === "bool"}
          <Toggle
            label={field.label}
            description={field.description}
            checked={field.value}
            onchange={(v) => patch(field.key, v)}
          />
        {:else if field.kind === "cycle"}
          <OptionGroup
            label={field.label}
            description={field.description}
            options={field.options}
            value={field.value}
            onchange={(v) => patch(field.key, v)}
          />
        {:else if field.kind === "text"}
          <SettingsTextField
            label={field.label}
            description={field.description}
            placeholder={field.placeholder}
            value={field.value}
            strict={field.strict}
            warning={textWarnings[field.key] ?? null}
            oncommit={(v) => {
              validateText(field.key, v);
              void patch(field.key, v);
            }}
          />
        {:else if field.kind === "listEntry"}
          <div class="flex items-center gap-2 py-1.5 font-mono text-xs text-fg-2">
            <span class="flex-1 truncate">{field.label}</span>
            <button
              type="button"
              class="text-muted hover:text-red-400 px-2"
              title="Remove"
              onclick={() => patch("watched.paths.remove", field.index)}
            >
              ×
            </button>
          </div>
        {:else if field.kind === "listAdd"}
          <div class="py-2 flex gap-2">
            <input
              type="text"
              class="flex-1 bg-surface border border-hairline rounded-md px-2 py-1.5 text-sm font-mono"
              placeholder="Glob pattern, e.g. .work/**"
              bind:value={addPattern}
            />
            <Button
              onclick={() => {
                const p = addPattern.trim();
                if (!p) return;
                void patch("watched.paths.add", p).then(() => {
                  addPattern = "";
                });
              }}
            >
              Add
            </Button>
          </div>
        {/if}
      {/each}

      {#if activeTab === "general"}
        {#if providers.length > 0}
          <h2 class="text-xs uppercase tracking-wider text-muted font-semibold mt-8 mb-2">AI Hub</h2>
          <p class="text-xs text-muted mb-3">Session selection. Save defaults writes to config TOML.</p>
          <div class="py-2">
            <div class="text-sm text-fg mb-1.5">Provider</div>
            <div class="flex flex-wrap gap-1">
              {#each providers as p (p.id)}
                <button
                  type="button"
                  class="px-2.5 py-1 text-xs rounded-md border transition-colors {p.is_selected
                    ? 'bg-accent text-black border-accent'
                    : 'bg-surface text-fg-2 border-hairline hover:bg-hover'}"
                  onclick={() => void selectProvider(p.id)}
                >
                  {p.label}
                </button>
              {/each}
            </div>
          </div>
          {#if selectedModels.length > 0}
            <div class="py-2">
              <div class="text-sm text-fg mb-1.5">Model</div>
              <div class="flex flex-wrap gap-1">
                {#each selectedModels as m (m.id)}
                  <button
                    type="button"
                    class="px-2.5 py-1 text-xs rounded-md border transition-colors {m.is_selected
                      ? 'bg-accent text-black border-accent'
                      : 'bg-surface text-fg-2 border-hairline hover:bg-hover'}"
                    onclick={() => void selectModel(m.id)}
                  >
                    {m.label}
                  </button>
                {/each}
              </div>
            </div>
          {/if}
          <div class="mt-2">
            <Button onclick={() => void saveDefaults()}>Save as default</Button>
          </div>
        {:else}
          <h2 class="text-xs uppercase tracking-wider text-muted font-semibold mt-8 mb-2">AI Hub</h2>
          <p class="text-xs text-muted">
            No <code class="font-mono">[ai_hub]</code> providers in config. Add providers in
            <code class="font-mono">.er-config.toml</code>.
          </p>
        {/if}
      {/if}

      {#if activeTab === "app"}
        <p class="text-xs text-muted mt-6">
          Diff layout (unified/split) is in the diff view gear menu.
        </p>
      {/if}
    </div>

    <footer class="shrink-0 flex flex-wrap gap-2 px-4 py-3 border-t border-hairline bg-surface">
      <Button onclick={() => void saveLocal()}>Save to repo</Button>
      <Button onclick={() => void saveGlobal()}>Save globally</Button>
      <button
        type="button"
        class="px-3 py-1.5 text-sm text-fg-3 hover:text-fg rounded-md hover:bg-hover"
        onclick={() => void revert()}
      >
        Revert
      </button>
    </footer>
  {/if}
</div>
