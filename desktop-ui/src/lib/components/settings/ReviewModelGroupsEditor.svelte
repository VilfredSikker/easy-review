<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AiProviderInfo, GetConfigHubResponse, ReviewModelGroups, ReviewModelRef } from "$lib/types";
  import SettingsButton from "./SettingsButton.svelte";
  import {
    cloneReviewModelGroups,
    hasReviewModel,
    reviewModelKey,
    sameReviewModelGroups,
    toggleReviewModel,
    type ReviewModelGroup,
  } from "./reviewModelGroups";

  interface Props {
    providers: AiProviderInfo[];
    reviewModels: ReviewModelGroups;
    onUpdated: (res: GetConfigHubResponse) => void;
    onError: (msg: string) => void;
  }

  let { providers, reviewModels, onUpdated, onError }: Props = $props();
  let draft = $state<ReviewModelGroups>(untrack(() => cloneReviewModelGroups(reviewModels)));
  let hasLocalEdits = $state(false);
  let saving = $state(false);
  let saveError = $state("");
  let previousReviewModels = untrack(() => reviewModels);

  const groupDetails: { id: ReviewModelGroup; title: string; usage: string }[] = [
    { id: "fast", title: "Fast", usage: "triage and Quick" },
    { id: "standard", title: "Standard", usage: "most reviews" },
    { id: "advanced", title: "Advanced", usage: "Deep, verification and the arbiter" },
  ];

  const dirty = $derived(!sameReviewModelGroups(draft, reviewModels));
  const availableKeys = $derived(
    new Set(
      providers.flatMap((provider) =>
        provider.models.map((model) => reviewModelKey({ provider_id: provider.id, model_id: model.id })),
      ),
    ),
  );

  function chips(group: ReviewModelGroup): { ref: ReviewModelRef; key: string; label: string; available: boolean }[] {
    return draft[group].map((ref) => {
      const provider = providers.find((item) => item.id === ref.provider_id);
      const model = provider?.models.find((item) => item.id === ref.model_id);
      return {
        ref,
        key: reviewModelKey(ref),
        label: provider && model ? `${provider.label} / ${model.label}` : `${ref.provider_id} / ${ref.model_id}`,
        available: availableKeys.has(reviewModelKey(ref)),
      };
    });
  }

  /** Models the group does not hold yet, grouped by provider for the add picker. */
  function addable(group: ReviewModelGroup) {
    return providers
      .map((provider) => ({
        provider,
        models: provider.models.filter(
          (model) => !hasReviewModel(draft, group, { provider_id: provider.id, model_id: model.id }),
        ),
      }))
      .filter((entry) => entry.models.length > 0);
  }

  // Refreshing config while a draft is open must not erase the user's edits.
  $effect(() => {
    const incoming = reviewModels;
    if (incoming === previousReviewModels) return;
    previousReviewModels = incoming;
    if (hasLocalEdits && !sameReviewModelGroups(draft, incoming)) return;
    hasLocalEdits = false;
    draft = cloneReviewModelGroups(incoming);
    saveError = "";
  });

  function toggle(group: ReviewModelGroup, ref: ReviewModelRef) {
    const next = toggleReviewModel(draft, group, ref);
    draft = sameReviewModelGroups(next, reviewModels) ? cloneReviewModelGroups(reviewModels) : next;
    hasLocalEdits = !sameReviewModelGroups(draft, reviewModels);
    saveError = "";
  }

  function addFromPicker(group: ReviewModelGroup, select: HTMLSelectElement) {
    const [providerId, modelId] = select.value.split("\u0000");
    select.value = "";
    if (providerId && modelId) toggle(group, { provider_id: providerId, model_id: modelId });
  }

  async function save() {
    if (saving || !dirty) return;
    saving = true;
    saveError = "";
    try {
      const res = await invoke<GetConfigHubResponse>("set_review_flow_models", {
        groups: cloneReviewModelGroups(draft),
      });
      hasLocalEdits = false;
      onUpdated(res);
    } catch (error) {
      saveError = String(error);
      onError(`Could not save Review model groups. ${error}`);
    } finally {
      saving = false;
    }
  }
</script>

<div class="grid grid-cols-1 min-[880px]:grid-cols-3">
  {#each groupDetails as group, gi (group.id)}
    <div
      class="flex flex-wrap content-start gap-1.5 px-3.5 py-3 border-line {gi === 0
        ? ''
        : 'border-t min-[880px]:border-t-0 min-[880px]:border-l'}"
      role="group"
      aria-label="{group.title} models"
    >
      <div class="w-full grid text-xs mb-1">
        <b class="font-semibold text-fg">{group.title}</b>
        <span class="text-[11px] text-muted">{group.usage}</span>
      </div>
      {#each chips(group.id) as chip (chip.key)}
        <span
          class="inline-flex items-center gap-1 h-[22px] pl-2 pr-1 rounded-md bg-hover text-xs {chip.available
            ? 'text-fg-2'
            : 'text-muted line-through'}"
          title={chip.available ? chip.ref.model_id : "Not offered by any configured provider"}
        >
          <span class="truncate max-w-[180px]">{chip.label}</span>
          <button
            type="button"
            class="w-4 h-4 grid place-items-center rounded text-fg-3 hover:bg-ink-600 hover:text-fg"
            aria-label="Remove {chip.label} from {group.title}"
            disabled={saving}
            onclick={() => toggle(group.id, chip.ref)}
          >×</button>
        </span>
      {:else}
        <span class="text-xs text-muted">No models</span>
      {/each}
      {#if addable(group.id).length > 0}
        <label class="w-full">
          <span class="sr-only">Add a model to {group.title}</span>
          <select
            class="mt-1 h-[22px] max-w-full px-1.5 rounded-[5px] border border-border bg-card text-[11px] text-fg-3 hover:text-fg outline-none"
            disabled={saving}
            value=""
            onchange={(e) => addFromPicker(group.id, e.currentTarget as HTMLSelectElement)}
          >
            <option value="" disabled>+ Add model</option>
            {#each addable(group.id) as entry (entry.provider.id)}
              <optgroup label={entry.provider.label}>
                {#each entry.models as model (model.id)}
                  <option value={reviewModelKey({ provider_id: entry.provider.id, model_id: model.id })}>{model.label}</option>
                {/each}
              </optgroup>
            {/each}
          </select>
        </label>
      {/if}
    </div>
  {/each}
</div>

<div class="flex flex-wrap items-center gap-2 px-3.5 py-2.5 border-t border-line">
  <span class="mr-auto text-xs text-fg-3">
    {#if saveError}
      <span class="text-error" role="alert">Could not save. {saveError}</span>
    {:else if dirty}
      Unsaved changes. Groups apply to the Review flow only.
    {:else}
      Only installed providers run; sign in through the provider CLI.
    {/if}
  </span>
  <SettingsButton disabled={!dirty || saving} onclick={() => void save()}>
    {saving ? "Saving…" : "Save groups"}
  </SettingsButton>
</div>
