<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import type {
    AiProviderInfo,
    ConfigFieldValue,
    ConfigHubField,
    GetConfigHubResponse,
    RefreshAiModelsResponse,
    ReviewModelGroups,
  } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import SettingsButton from "./SettingsButton.svelte";
  import Segmented from "./Segmented.svelte";
  import Switch from "./Switch.svelte";
  import FieldRows from "./FieldRows.svelte";
  import AiProviderEditor from "./AiProviderEditor.svelte";
  import ReviewModelGroupsEditor from "./ReviewModelGroupsEditor.svelte";
  import { INPUT_CLASS, fieldMap, optionLabel, pickFields } from "./settingsModel";
  import {
    AGENT_DESCRIPTIONS,
    AGENT_KEYS,
    AI_ROWS,
    CONCURRENCY_DESCRIPTIONS,
    CONCURRENCY_KEYS,
    MODEL_GROUP_KEYWORDS,
  } from "./settingsSearch";

  interface Props {
    fields: ConfigHubField[];
    providers: AiProviderInfo[];
    reviewModels: ReviewModelGroups;
    familyOptions: string[];
    activeEffort: string;
    warnings: Record<string, string | null>;
    onpatch: (key: string, value: ConfigFieldValue) => Promise<void>;
    onreload: () => Promise<void>;
    onUpdated: (res: GetConfigHubResponse) => void;
    onProviders: (providers: AiProviderInfo[]) => void;
  }

  const {
    fields,
    providers,
    reviewModels,
    familyOptions,
    activeEffort,
    warnings,
    onpatch,
    onreload,
    onUpdated,
    onProviders,
  }: Props = $props();

  const byKey = $derived(fieldMap(fields));
  const agentFields = $derived(pickFields(byKey, AGENT_KEYS));
  const concurrencyFields = $derived(pickFields(byKey, CONCURRENCY_KEYS));
  const discovery = $derived.by(() => {
    const field = byKey.get("features.model_discovery");
    return field?.kind === "bool" ? field : null;
  });

  const selectedProvider = $derived(providers.find((p) => p.is_selected) ?? null);
  const models = $derived(selectedProvider?.models ?? []);
  const selectedModel = $derived(models.find((m) => m.is_selected) ?? null);
  const effortOptions = $derived(["Auto", ...(selectedModel?.effort_levels ?? [])]);
  // A model without the saved level reads as Auto, which is what the provider
  // will actually run.
  const effort = $derived(effortOptions.includes(activeEffort) ? activeEffort : "Auto");

  let editProviders = $state(false);
  let refreshing = $state(false);

  async function selectProvider(providerId: string) {
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) return;
    const model = provider.models.find((m) => m.is_selected) ?? provider.models[0];
    await invoke("set_ai_selection", { providerId, modelId: model?.id ?? null, persist: true });
    await onreload();
  }

  async function selectModel(modelId: string) {
    if (!selectedProvider) return;
    await invoke("set_ai_selection", { providerId: selectedProvider.id, modelId, persist: true });
    await onreload();
  }

  async function selectEffort(level: string) {
    try {
      await invoke("set_ai_effort", { effort: level === "Auto" ? null : level, persist: true });
      await onreload();
    } catch (e) {
      app.showToast("error", `set_ai_effort: ${e}`);
    }
  }

  async function refreshModels() {
    const provider = selectedProvider;
    if (!provider) return;
    // Claude's CLI has no model listing, so its models are the ones er ships.
    if (!provider.has_models_command) {
      app.showToast("info", `${provider.label} has no model listing command; its models ship with Easy Review.`);
      return;
    }
    refreshing = true;
    try {
      const res = await invoke<RefreshAiModelsResponse>("refresh_ai_models", { providerId: provider.id, force: true });
      onProviders(res.providers);
      if (res.errors.length > 0) {
        for (const err of res.errors) app.showToast("error", err);
      } else {
        const count = res.providers.find((p) => p.id === provider.id)?.models.length ?? 0;
        app.showToast("success", `${provider.label}: ${count} models`);
      }
    } catch (e) {
      app.showToast("error", `refresh_ai_models: ${e}`);
    } finally {
      refreshing = false;
    }
  }
</script>

<SettingsSection title="Default model">
  {#if providers.length === 0}
    <SettingsRow {...AI_ROWS.noProviders}>
      <SettingsButton onclick={() => (editProviders = !editProviders)}>Edit providers…</SettingsButton>
    </SettingsRow>
  {:else}
    <SettingsRow {...AI_ROWS.provider}>
      <Segmented
        options={providers.map((p) => p.id)}
        value={selectedProvider?.id ?? ""}
        ariaLabel="Provider"
        display={(id) => providers.find((p) => p.id === id)?.label ?? id}
        onchange={(id) => void selectProvider(id)}
      />
    </SettingsRow>
    {#if models.length > 0}
      <SettingsRow {...AI_ROWS.model}>
        <select
          class="{INPUT_CLASS} font-sans max-w-[280px]"
          aria-label="Model"
          value={selectedModel?.id ?? ""}
          onchange={(e) => void selectModel((e.currentTarget as HTMLSelectElement).value)}
        >
          {#if !selectedModel}<option value="" disabled>Pick a model</option>{/if}
          {#each models as model (model.id)}
            <option value={model.id}>{model.label}</option>
          {/each}
        </select>
      </SettingsRow>
    {/if}
    <SettingsRow {...AI_ROWS.effort}>
      <Segmented
        options={effortOptions}
        value={effort}
        ariaLabel="Effort"
        display={(o) => (o === "Auto" ? o : optionLabel(o))}
        onchange={(o) => void selectEffort(o)}
      />
    </SettingsRow>
    <SettingsRow {...AI_ROWS.discovery}>
      {#if discovery}
        {@const field = discovery}
        <Switch label="Model discovery" checked={field.value} onchange={(v) => void onpatch(field.key, v)} />
      {/if}
      <SettingsButton disabled={refreshing} onclick={() => void refreshModels()}>
        {refreshing ? "Refreshing…" : "Refresh models"}
      </SettingsButton>
    </SettingsRow>
    <SettingsRow {...AI_ROWS.providers}>
      <SettingsButton onclick={() => (editProviders = !editProviders)}>
        {editProviders ? "Hide provider editor" : "Edit providers…"}
      </SettingsButton>
    </SettingsRow>
  {/if}
  {#if editProviders}
    <SettingsRow always layout="col">
      <AiProviderEditor
        {providers}
        {familyOptions}
        {onUpdated}
        onError={(msg) => app.showToast("error", msg)}
      />
    </SettingsRow>
  {/if}
</SettingsSection>

<SettingsSection title="Model groups" keywords={MODEL_GROUP_KEYWORDS}>
  {#snippet aside()}Which models each review stage may use{/snippet}
  <ReviewModelGroupsEditor {providers} {reviewModels} {onUpdated} onError={(msg) => app.showToast("error", msg)} />
</SettingsSection>

{#if agentFields.length > 0}
  <SettingsSection title="Agent">
    <FieldRows fields={agentFields} {warnings} {onpatch} descriptions={AGENT_DESCRIPTIONS} />
  </SettingsSection>
{/if}

{#if concurrencyFields.length > 0}
  <SettingsSection id="settings-concurrency" title="Concurrency">
    <FieldRows fields={concurrencyFields} {warnings} {onpatch} descriptions={CONCURRENCY_DESCRIPTIONS} />
  </SettingsSection>
{/if}
