<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import type {
    AiProviderInfo,
    AppSnapshot,
    ConfigFieldValue,
    ConfigHubField,
    GetConfigHubResponse,
    SettingsTab,
  } from "$lib/types";
  import Toggle from "./Toggle.svelte";
  import OptionGroup from "./OptionGroup.svelte";
  import SettingsTextField from "./SettingsTextField.svelte";
  import ProjectSettingsCard from "./ProjectSettingsCard.svelte";
  import Button from "$lib/components/ui/Button.svelte";

  interface Props {
    onBack: () => void;
  }

  const { onBack }: Props = $props();

  const TABS: { id: SettingsTab; label: string; blurb: string }[] = [
    { id: "general", label: "General", blurb: "Shared config for both app and terminal." },
    {
      id: "projects",
      label: "Projects",
      blurb: "Per-project triage timing, size limits, and review ignore patterns.",
    },
    { id: "terminal", label: "Terminal", blurb: "TUI (`er`) only — view modes, display, and key hints." },
  ];

  let loading = $state(true);
  let activeTab = $state<SettingsTab>("general");
  let generalFields = $state<ConfigHubField[]>([]);
  let terminalFields = $state<ConfigHubField[]>([]);
  let providers = $state<AiProviderInfo[]>([]);
  let selectedEffort = $state("Auto");
  let repoRoot = $state("");
  let addPattern = $state("");
  let textWarnings = $state<Record<string, string | null>>({});

  let uninstallPreview = $state<{
    targets: { kind: string; path: string; exists: boolean; description: string }[];
    existingCount: number;
  } | null>(null);
  let uninstallLoading = $state(false);
  let uninstallConfirm = $state(false);
  let uninstallTyped = $state("");
  let uninstallBusy = $state(false);
  let focusUninstall = $state(false);

  const fields = $derived(activeTab === "general" ? generalFields : terminalFields);

  const tabBlurb = $derived(TABS.find((t) => t.id === activeTab)?.blurb ?? "");

  const projects = $derived(app.snapshot?.projects ?? []);

  const selectedProvider = $derived(providers.find((p) => p.is_selected) ?? null);
  const selectedModels = $derived(selectedProvider?.models ?? []);
  const selectedModel = $derived(selectedModels.find((model) => model.is_selected) ?? null);
  const effortOptions = $derived(["Auto", ...(selectedModel?.effort_levels ?? [])]);
  interface FieldSection {
    title: string | null;
    fields: ConfigHubField[];
  }

  const sections = $derived.by<FieldSection[]>(() => {
    const out: FieldSection[] = [];
    let current: FieldSection = { title: null, fields: [] };
    for (const field of fields) {
      if (field.kind === "section") {
        if (current.fields.length > 0) out.push(current);
        current = { title: field.title, fields: [] };
      } else {
        current.fields.push(field);
      }
    }
    if (current.fields.length > 0) out.push(current);
    return out;
  });

  function fieldKey(field: ConfigHubField, i: number): string {
    if (field.kind === "bool" || field.kind === "cycle" || field.kind === "text") return field.key;
    if (field.kind === "listEntry") return field.key + "-" + field.index;
    if (field.kind === "section") return "section-" + field.title;
    return "field-" + i;
  }

  function applySettings(res: GetConfigHubResponse) {
    generalFields = res.settings.general;
    terminalFields = res.settings.terminal;
    providers = res.providers;
    selectedEffort = res.activeEffort ?? "Auto";
    if (!effortOptions.includes(selectedEffort)) selectedEffort = "Auto";
    repoRoot = res.settings.repoRoot;
  }

  async function reload() {
    loading = true;
    try {
      const res = await invoke<GetConfigHubResponse>("get_config_hub", {});
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
      await invoke("set_ai_selection", { providerId, modelId: null, persist: true });
      await reload();
      return;
    }
    const model = p.models.find((m) => m.is_selected) ?? p.models[0];
    await invoke("set_ai_selection", { providerId, modelId: model?.id ?? null, persist: true });
    await reload();
  }

  async function selectModel(modelId: string) {
    const p = selectedProvider;
    if (!p) return;
    await invoke("set_ai_selection", { providerId: p.id, modelId, persist: true });
    const model = p.models.find((item) => item.id === modelId);
    if (!model?.effort_levels.includes(selectedEffort)) selectedEffort = "Auto";
    await reload();
  }

  async function selectEffort(level: string) {
    selectedEffort = level;
    try {
      await invoke("set_ai_effort", {
        effort: level === "Auto" ? null : level,
        persist: true,
      });
      await reload();
    } catch (e) {
      app.showToast("error", `set_ai_effort: ${e}`);
    }
  }

  async function patchProjectReviewSettings(
    projectId: string,
    patch: Record<string, unknown>,
  ) {
    try {
      const snap = await invoke<AppSnapshot>("patch_project_review_settings", {
        projectId,
        patch,
      });
      app.ingestCommandSnapshot(snap);
    } catch (e) {
      app.showToast("error", `patch_project_review_settings: ${e}`);
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

  async function loadUninstallPreview() {
    uninstallLoading = true;
    try {
      const res = await invoke<{
        targets: { kind: string; path: string; exists: boolean; description: string }[];
        existingCount: number;
      }>("preview_uninstall", { request: null });
      uninstallPreview = res;
    } catch (e) {
      app.showToast("error", `preview_uninstall: ${e}`);
    } finally {
      uninstallLoading = false;
    }
  }

  async function confirmUninstall() {
    if (uninstallTyped.trim() !== "uninstall") return;
    uninstallBusy = true;
    try {
      await invoke("run_uninstall", { request: null });
      app.showToast("success", "Easy Review removed — quitting…");
    } catch (e) {
      app.showToast("error", String(e));
      uninstallBusy = false;
      await loadUninstallPreview();
    }
  }

  $effect(() => {
    void reload();
  });

  onMount(() => {
    try {
      if (localStorage.getItem("er.focusUninstall") === "1") {
        localStorage.removeItem("er.focusUninstall");
        activeTab = "general";
        focusUninstall = true;
      }
    } catch {
      /* ignore */
    }
  });

  // Scroll + preview after settings finish loading (section missing while loading).
  $effect(() => {
    if (!focusUninstall || loading || activeTab !== "general") return;
    focusUninstall = false;
    void loadUninstallPreview().then(() => {
      queueMicrotask(() => {
        document
          .getElementById("settings-uninstall")
          ?.scrollIntoView({ behavior: "smooth", block: "start" });
      });
    });
  });
</script>

<div class="flex flex-col flex-1 w-full min-w-0 h-full min-h-0 bg-bg text-fg">
  <header class="shrink-0 flex items-center gap-3 px-5 py-3 border-b border-hairline bg-surface/60">
    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-2 py-1 -ml-2 text-sm text-fg-3 hover:text-fg hover:bg-hover rounded-md transition-colors"
      onclick={onBack}
    >
      <span aria-hidden="true">←</span>
      Back
    </button>
    <div class="w-px h-4 bg-hairline" aria-hidden="true"></div>
    <h1 class="text-base font-semibold tracking-tight">Settings</h1>
    <span
      class="ml-auto inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-ink-650 border border-border text-[10px] font-mono text-muted truncate"
      title={repoRoot}
    >
      <span
        class="w-1.5 h-1.5 rounded-full shrink-0 bg-ink-300"
        aria-hidden="true"
      ></span>
      global defaults
    </span>
  </header>

  {#if loading}
    <div class="flex-1 overflow-hidden px-6 py-6">
      <div class="max-w-2xl mx-auto space-y-4 animate-pulse" aria-label="Loading settings">
        <div class="h-7 w-64 bg-ink-700 rounded-lg"></div>
        <div class="h-3 w-80 bg-ink-750 rounded"></div>
        <div class="h-40 bg-ink-800 border border-hairline rounded-xl"></div>
        <div class="h-40 bg-ink-800 border border-hairline rounded-xl"></div>
      </div>
    </div>
  {:else}
    <div class="flex-1 overflow-y-auto w-full min-h-0">
      <div class="max-w-2xl mx-auto px-6 py-5 pb-8">
        <div
          class="inline-flex p-0.5 gap-0.5 bg-surface border border-hairline rounded-lg"
          role="tablist"
          aria-label="Settings sections"
        >
          {#each TABS as tab (tab.id)}
            <button
              type="button"
              role="tab"
              aria-selected={activeTab === tab.id}
              class="px-3.5 py-1 text-xs rounded-md transition-colors {activeTab === tab.id
                ? 'bg-hover text-fg font-medium shadow-sm'
                : 'text-fg-3 hover:text-fg'}"
              onclick={() => (activeTab = tab.id)}
            >
              {tab.label}
            </button>
          {/each}
        </div>
        <p class="text-xs text-muted mt-3 mb-5">{tabBlurb}</p>

        {#if activeTab === "projects"}
          {#if projects.length === 0}
            <div class="border border-dashed border-border rounded-xl px-6 py-10 text-center">
              <p class="text-sm text-fg-3 mb-1">No projects registered yet</p>
              <p class="text-xs text-muted">Open a repo from the sidebar first.</p>
            </div>
          {:else}
            {#each projects as project (project.id)}
              <ProjectSettingsCard
                {project}
                onpatch={(patch) => patchProjectReviewSettings(project.id, patch)}
              />
            {/each}
          {/if}
        {:else}
          {#each sections as section, si (section.title ?? "untitled-" + si)}
            {#if section.title}
              <h2 class="flex items-center gap-2 text-xs uppercase tracking-wider text-muted font-semibold mt-7 mb-2.5 first:mt-0">
                <span class="w-1 h-3 rounded-full bg-accent/70" aria-hidden="true"></span>
                {section.title}
              </h2>
            {/if}
            <div class="bg-card border border-hairline rounded-xl divide-y divide-hairline/60">
              {#each section.fields as field, i (fieldKey(field, i))}
                {#if field.kind === "bool"}
                  <div class="px-4">
                    <Toggle
                      label={field.label}
                      description={field.description}
                      checked={field.value}
                      onchange={(v) => patch(field.key, v)}
                    />
                  </div>
                {:else if field.kind === "cycle"}
                  <div class="px-4">
                    <OptionGroup
                      label={field.label}
                      description={field.description}
                      options={field.options}
                      value={field.value}
                      onchange={(v) => patch(field.key, v)}
                    />
                  </div>
                {:else if field.kind === "text"}
                  <div class="px-4">
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
                  </div>
                {:else if field.kind === "listEntry"}
                  <div class="flex items-center gap-2 px-4 py-2 font-mono text-xs text-fg-2 group">
                    <span class="flex-1 truncate" title={field.label}>{field.label}</span>
                    <button
                      type="button"
                      class="text-muted opacity-60 group-hover:opacity-100 hover:text-risk-high px-2 py-0.5 rounded transition-colors"
                      title="Remove"
                      onclick={() => patch("watched.paths.remove", field.index)}
                    >
                      ×
                    </button>
                  </div>
                {:else if field.kind === "listAdd"}
                  <div class="px-4 py-3 flex gap-2">
                    <input
                      type="text"
                      class="flex-1 bg-ink-850 border border-hairline rounded-md px-2.5 py-1.5 text-sm font-mono outline-none transition-colors focus:border-accent/60 placeholder:text-ink-300"
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
            </div>
          {/each}
        {/if}

        {#if activeTab === "general"}
          <h2 class="flex items-center gap-2 text-xs uppercase tracking-wider text-muted font-semibold mt-7 mb-2.5">
            <span class="w-1 h-3 rounded-full bg-accent/70" aria-hidden="true"></span>
            AI Hub
          </h2>
          {#if providers.length > 0}
            <div class="bg-card border border-hairline rounded-xl px-4 py-3">
              <p class="text-xs text-muted mb-3">
                The default provider, model, and reasoning effort for all AI Hub actions. Changes
                save to config immediately. Mid-session picks in the AI action palette stay
                session-only.
              </p>
              <div class="py-1">
                <div class="text-sm text-fg mb-1.5">Provider</div>
                <div class="flex flex-wrap gap-1.5">
                  {#each providers as p (p.id)}
                    <button
                      type="button"
                      class="px-2.5 py-1 text-xs rounded-md border transition-colors {p.is_selected
                        ? 'bg-accent-soft text-accent border-accent-border font-medium'
                        : 'bg-surface text-fg-2 border-hairline hover:bg-hover hover:border-border'}"
                      onclick={() => void selectProvider(p.id)}
                    >
                      {p.label}
                    </button>
                  {/each}
                </div>
              </div>
              {#if selectedModels.length > 0}
                <div class="py-2 mt-1">
                  <div class="text-sm text-fg mb-1.5">Model</div>
                  <div class="flex flex-wrap gap-1.5">
                    {#each selectedModels as m (m.id)}
                      <button
                        type="button"
                        class="px-2.5 py-1 text-xs rounded-md border transition-colors {m.is_selected
                          ? 'bg-accent-soft text-accent border-accent-border font-medium'
                          : 'bg-surface text-fg-2 border-hairline hover:bg-hover hover:border-border'}"
                        onclick={() => void selectModel(m.id)}
                      >
                        {m.label}
                      </button>
                    {/each}
                  </div>
                </div>
              {/if}
              <div class="py-2 mt-1">
                <div class="text-sm text-fg mb-1.5">Effort / reasoning</div>
                <div class="flex flex-wrap gap-1.5">
                  {#each effortOptions as level (level)}
                    <button
                      type="button"
                      class="px-2.5 py-1 text-xs rounded-md border transition-colors {selectedEffort === level
                        ? 'bg-accent-soft text-accent border-accent-border font-medium'
                        : 'bg-surface text-fg-2 border-hairline hover:bg-hover hover:border-border'}"
                      onclick={() => void selectEffort(level)}
                    >
                      {level === 'xhigh' ? 'XHigh' : level}
                    </button>
                  {/each}
                </div>
                <p class="text-[11px] text-muted mt-1.5">Auto uses the provider default.</p>
              </div>
            </div>
          {:else}
            <div class="border border-dashed border-border rounded-xl px-6 py-8 text-center">
              <p class="text-xs text-muted">
                No <code class="font-mono">[ai_hub]</code> providers in config. Add providers in
                <code class="font-mono">~/.config/er/config.toml</code>.
              </p>
            </div>
          {/if}

          <h2
            id="settings-uninstall"
            class="flex items-center gap-2 text-xs uppercase tracking-wider text-muted font-semibold mt-7 mb-2.5 scroll-mt-4"
          >
            <span class="w-1 h-3 rounded-full bg-risk-high/70" aria-hidden="true"></span>
            Uninstall
          </h2>
          <div class="bg-card border border-hairline rounded-xl px-4 py-4 space-y-3">
            <p class="text-sm text-fg-3">
              Remove Easy Review from this machine: config, review data, legacy cache, and installed
              apps. Diff review of your repos is unaffected — only Easy Review’s own files are deleted.
            </p>
            {#if uninstallPreview}
              <ul class="space-y-1.5 text-xs font-mono text-fg-3 max-h-40 overflow-y-auto">
                {#each uninstallPreview.targets as t (t.path)}
                  <li class="flex gap-2">
                    <span class={t.exists ? "text-fg-2" : "text-muted"}>{t.exists ? "•" : "·"}</span>
                    <span class="min-w-0 break-all">{t.description}{t.exists ? "" : " (not present)"}</span>
                  </li>
                {/each}
              </ul>
            {:else if uninstallLoading}
              <p class="text-xs text-muted">Scanning install locations…</p>
            {/if}
            {#if !uninstallConfirm}
              <div class="flex flex-wrap gap-2 pt-1">
                <Button variant="ghost" onclick={() => void loadUninstallPreview()}>
                  {uninstallPreview ? "Refresh list" : "Show what will be removed"}
                </Button>
                <Button
                  variant="danger"
                  disabled={!uninstallPreview || uninstallPreview.existingCount === 0 || uninstallBusy}
                  onclick={() => (uninstallConfirm = true)}
                >
                  Uninstall…
                </Button>
              </div>
            {:else}
              <div class="rounded-lg border border-risk-high/40 bg-risk-high/5 px-3 py-3 space-y-2">
                <p class="text-sm text-fg-1">
                  Type <span class="font-mono text-risk-high">uninstall</span> to confirm. The app will quit afterward.
                </p>
                <input
                  type="text"
                  class="w-full bg-ink-850 border border-hairline rounded-md px-2.5 py-1.5 text-sm font-mono outline-none focus:border-risk-high/60"
                  placeholder="uninstall"
                  bind:value={uninstallTyped}
                  disabled={uninstallBusy}
                />
                <div class="flex gap-2">
                  <Button
                    variant="danger"
                    disabled={uninstallTyped.trim() !== "uninstall" || uninstallBusy}
                    onclick={() => void confirmUninstall()}
                  >
                    {uninstallBusy ? "Removing…" : "Remove everything"}
                  </Button>
                  <Button
                    variant="ghost"
                    disabled={uninstallBusy}
                    onclick={() => {
                      uninstallConfirm = false;
                      uninstallTyped = "";
                    }}
                  >
                    Cancel
                  </Button>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <footer class="shrink-0 flex items-center px-5 py-2 border-t border-hairline bg-surface">
      <span class="max-w-2xl mx-auto w-full px-1 text-[10px] font-mono text-muted">
        Saved automatically to ~/.config/er/config.toml
      </span>
    </footer>
  {/if}
</div>
