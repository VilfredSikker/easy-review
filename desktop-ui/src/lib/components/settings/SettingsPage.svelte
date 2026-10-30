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
    ReviewModelGroups,
    SettingsTab,
  } from "$lib/types";
  import GeneralSettings from "./GeneralSettings.svelte";
  import AiSettings from "./AiSettings.svelte";
  import ReviewSettings from "./ReviewSettings.svelte";
  import ProjectsSettings from "./ProjectsSettings.svelte";
  import TerminalSettings from "./TerminalSettings.svelte";
  import { SETTINGS_TABS, buildSearchIndex, tabMatchCounts } from "./settingsModel";
  import { provideSettingsQuery } from "./settingsContext";
  import {
    RISK_RULE_PATCH_KEY,
    type RiskFileRow,
    type RiskRuleRow,
    type RiskSettingsPayload,
    type RiskTier,
  } from "./riskRules";

  interface Props {
    onBack: () => void;
  }

  const { onBack }: Props = $props();

  let loading = $state(true);
  let activeTab = $state<SettingsTab>("general");
  let query = $state("");
  let generalFields = $state<ConfigHubField[]>([]);
  let terminalFields = $state<ConfigHubField[]>([]);
  let providers = $state<AiProviderInfo[]>([]);
  let reviewModels = $state<ReviewModelGroups>({ fast: [], standard: [], advanced: [] });
  let familyOptions = $state<string[]>([]);
  let activeEffort = $state("Auto");
  let riskRepo = $state("");
  let riskRules = $state<RiskRuleRow[]>([]);
  let riskDefault = $state<RiskTier>("ai");
  let riskFiles = $state<RiskFileRow[]>([]);
  const textWarnings = $state<Record<string, string | null>>({});
  let testBusy = $state(false);
  let focusUninstall = $state(false);

  provideSettingsQuery(() => query);

  const tab = $derived(SETTINGS_TABS.find((t) => t.id === activeTab) ?? SETTINGS_TABS[0]);
  const projects = $derived(app.snapshot?.projects ?? []);
  const searchIndex = $derived(buildSearchIndex(generalFields, terminalFields));
  const matchCounts = $derived(tabMatchCounts(searchIndex, query));
  const otherMatches = $derived(
    query.trim() && matchCounts[activeTab] === 0 ? SETTINGS_TABS.filter((t) => matchCounts[t.id] > 0) : [],
  );

  function applySettings(res: GetConfigHubResponse) {
    const settings: GetConfigHubResponse["settings"] & RiskSettingsPayload = res.settings;
    generalFields = settings.general;
    terminalFields = settings.terminal;
    providers = res.providers;
    reviewModels = res.reviewModels;
    familyOptions = res.familyOptions ?? [];
    activeEffort = res.activeEffort ?? "Auto";
    riskRepo = settings.riskRepo ?? "";
    riskRules = settings.riskRules ?? [];
    riskDefault = settings.riskDefault ?? "ai";
    riskFiles = settings.riskFiles ?? [];
    for (const w of res.warnings ?? []) {
      app.showToast("info", w);
    }
  }

  // `loading` starts true and is never set again: flipping it swaps the scroll
  // container for the skeleton, and the new container mounts at the top, so
  // every pick in the AI tab would scroll the page away from it.
  async function reload() {
    try {
      applySettings(await invoke<GetConfigHubResponse>("get_config_hub", {}));
    } catch (e) {
      app.showToast("error", `get_config_hub: ${e}`);
    } finally {
      loading = false;
    }
  }

  function validateText(key: string, value: ConfigFieldValue) {
    if (typeof value !== "string") return;
    if (key === "agent.args" && !value.includes("{prompt}")) {
      textWarnings[key] = "Include {prompt} in args so the agent receives user input.";
    } else if (key === "agent.command" && !value.trim()) {
      textWarnings[key] = "Command cannot be empty.";
    } else {
      textWarnings[key] = null;
    }
  }

  async function send(key: string, value: ConfigFieldValue): Promise<GetConfigHubResponse | null> {
    validateText(key, value);
    try {
      const res = await invoke<GetConfigHubResponse>("apply_config_patch", { patch: { key, value } });
      applySettings(res);
      return res;
    } catch (e) {
      app.showToast("error", `apply_config_patch: ${e}`);
      return null;
    }
  }

  async function patch(key: string, value: ConfigFieldValue): Promise<void> {
    await send(key, value);
  }

  async function editRiskRule(edit: string): Promise<RiskRuleRow[] | null> {
    const res = await send(RISK_RULE_PATCH_KEY, edit);
    if (!res) return null;
    return (res.settings as GetConfigHubResponse["settings"] & RiskSettingsPayload).riskRules ?? [];
  }

  // Hands off to the agent and returns as soon as the task is queued — it reads
  // the whole repo, so the table lands minutes later and the tabs pick it up
  // through the task poll. `app.cmd` ingests the returned snapshot; a raw
  // `invoke` would discard it and swallow any failure with it.
  async function proposeRiskRules() {
    await app.cmd("run_importance_agent");
  }

  async function sendTestNotification() {
    testBusy = true;
    try {
      await invoke("test_native_notification");
      app.showToast("info", "Test notification sent");
    } catch (e) {
      app.showToast("error", `test_native_notification: ${e}`);
    } finally {
      testBusy = false;
    }
  }

  async function patchProject(projectId: string, changes: Record<string, unknown>) {
    try {
      const snap = await invoke<AppSnapshot>("patch_project_review_settings", { projectId, patch: changes });
      app.ingestCommandSnapshot(snap);
    } catch (e) {
      app.showToast("error", `patch_project_review_settings: ${e}`);
    }
  }

  function selectTab(id: SettingsTab) {
    activeTab = id;
    document.getElementById("settings-main")?.scrollTo({ top: 0 });
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
</script>

<div
  class="grid grid-cols-1 grid-rows-[auto_minmax(0,1fr)] min-[880px]:grid-cols-[230px_minmax(0,1fr)] min-[880px]:grid-rows-1 flex-1 w-full min-w-0 h-full min-h-0 bg-bg text-fg"
>
  <aside
    class="flex flex-row flex-wrap min-[880px]:flex-col gap-0.5 px-2.5 py-3.5 border-b min-[880px]:border-b-0 min-[880px]:border-r border-line overflow-auto"
    aria-label="Settings sections"
  >
    <button
      type="button"
      class="self-start inline-flex items-center gap-1.5 h-[26px] px-2.5 rounded-[7px] text-xs font-medium text-fg-3 hover:bg-hover hover:text-fg"
      onclick={onBack}
    >‹ Back to review</button>
    <label
      class="max-[879px]:hidden flex items-center gap-1.5 h-7 px-2 mt-2 mb-1.5 border border-line rounded-[7px] bg-card text-muted focus-within:border-border"
    >
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true">
        <circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" />
      </svg>
      <input
        class="flex-1 min-w-0 border-0 bg-transparent outline-none text-xs text-fg placeholder:text-muted"
        placeholder="Search settings…"
        aria-label="Search settings"
        bind:value={query}
        onkeydown={(e) => {
          e.stopPropagation();
          if (e.key === "Escape") query = "";
        }}
      />
    </label>
    <div role="tablist" aria-orientation="vertical" class="contents">
      {#each SETTINGS_TABS as item (item.id)}
        {@const on = item.id === activeTab}
        <button
          type="button"
          role="tab"
          aria-selected={on}
          class="flex items-center gap-2.5 px-2.5 py-[7px] rounded-[7px] text-[13px] font-medium text-left transition-colors {on
            ? 'bg-hover text-fg ring-1 ring-inset ring-line'
            : 'text-fg-3 hover:bg-hover hover:text-fg'}"
          onclick={() => selectTab(item.id)}
        >
          <span class="w-[18px] text-center text-xs font-mono {on ? 'text-accent' : 'text-muted'}" aria-hidden="true">{item.icon}</span>
          {item.label}
          {#if query.trim() && matchCounts[item.id] > 0}
            <span class="ml-auto text-[10.5px] font-mono text-muted" title="Matching settings">{matchCounts[item.id]}</span>
          {/if}
        </button>
      {/each}
    </div>
    <div class="max-[879px]:hidden mt-auto p-2.5 grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5 items-center text-xs">
      <span class="w-[7px] h-[7px] rounded-full bg-success" aria-hidden="true"></span>
      <span class="text-fg">Global defaults</span>
      <span class="col-span-2 text-[11px] leading-[1.45] text-muted">
        Saved automatically to managed app data. No repo-local config.
      </span>
    </div>
  </aside>

  <div id="settings-main" class="overflow-auto min-h-0">
    {#if loading}
      <div class="max-w-[760px] mx-auto px-7 pt-7 space-y-4 animate-pulse" aria-label="Loading settings">
        <div class="h-6 w-40 bg-hover rounded"></div>
        <div class="h-3 w-80 bg-hover rounded"></div>
        <div class="h-40 bg-card border border-line rounded-[10px]"></div>
        <div class="h-40 bg-card border border-line rounded-[10px]"></div>
      </div>
    {:else}
      <div class="max-w-[760px] mx-auto px-7 pt-7 pb-12" role="tabpanel" aria-label={tab.label}>
        <h2 class="m-0 text-[22px] font-semibold tracking-[-.015em]">{tab.label}</h2>
        <p class="mt-1 mb-6 text-[13px] text-fg-3">{tab.blurb}</p>

        {#if otherMatches.length > 0}
          <p class="mb-4 text-xs text-fg-3">
            Nothing on {tab.label} matches “{query.trim()}”. Found on
            {#each otherMatches as other, i (other.id)}{i > 0 ? ", " : " "}<button
                type="button"
                class="text-accent hover:underline"
                onclick={() => selectTab(other.id)}>{other.label}</button
              >{/each}.
          </p>
        {/if}

        {#if activeTab === "general"}
          <GeneralSettings
            fields={generalFields}
            warnings={textWarnings}
            {testBusy}
            {focusUninstall}
            onpatch={patch}
            ontest={() => void sendTestNotification()}
          />
        {:else if activeTab === "ai"}
          <AiSettings
            fields={generalFields}
            {providers}
            {reviewModels}
            {familyOptions}
            {activeEffort}
            warnings={textWarnings}
            onpatch={patch}
            onreload={reload}
            onUpdated={applySettings}
            onProviders={(next) => (providers = next)}
          />
        {:else if activeTab === "review"}
          <ReviewSettings
            fields={generalFields}
            warnings={textWarnings}
            {riskRepo}
            {riskRules}
            {riskDefault}
            {riskFiles}
            onpatch={patch}
            onriskedit={editRiskRule}
            onpropose={() => void proposeRiskRules()}
          />
        {:else if activeTab === "projects"}
          <ProjectsSettings {projects} onpatch={patchProject} />
        {:else}
          <TerminalSettings fields={terminalFields} warnings={textWarnings} onpatch={patch} />
        {/if}

        <p class="mt-2 text-center font-mono text-[11px] text-muted">Settings are saved automatically to managed app data.</p>
      </div>
    {/if}
  </div>
</div>
