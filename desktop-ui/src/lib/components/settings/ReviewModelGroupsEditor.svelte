<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AiProviderInfo, GetConfigHubResponse, ReviewModelGroups, ReviewModelRef } from "$lib/types";
  import Button from "$lib/components/ui/Button.svelte";
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

  const groupDetails: {
    id: ReviewModelGroup;
    title: string;
    description: string;
  }[] = [
    { id: "fast", title: "Fast", description: "Triage and Quick reviews of changed files." },
    { id: "standard", title: "Standard", description: "The models used for most reviews." },
    {
      id: "advanced",
      title: "Advanced",
      description: "Deep review, verification, and arbiter work.",
    },
  ];

  const dirty = $derived(!sameReviewModelGroups(draft, reviewModels));
  const availableKeys = $derived(
    new Set(
      providers.flatMap((provider) =>
        provider.models.map((model) =>
          reviewModelKey({ provider_id: provider.id, model_id: model.id }),
        ),
      ),
    ),
  );

  function selectedModels(group: ReviewModelGroup): { key: string; label: string }[] {
    return draft[group].map((ref) => {
      const provider = providers.find((item) => item.id === ref.provider_id);
      const model = provider?.models.find((item) => item.id === ref.model_id);
      return {
        key: reviewModelKey(ref),
        label: provider && model ? `${provider.label} / ${model.label}` : `${ref.provider_id} / ${ref.model_id}`,
      };
    });
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

<section class="mt-3 border-t border-hairline pt-3" aria-labelledby="review-model-groups-title">
  <div class="mb-3">
    <h3 id="review-model-groups-title" class="text-sm font-medium text-fg">Review model groups</h3>
    <p class="mt-1 text-xs text-muted">
      These models are eligible for each Review flow stage. Only installed providers run. Sign in through the provider CLI.
      Standalone actions use the default model in Settings.
    </p>
  </div>

  <div class="space-y-2">
    {#each groupDetails as group (group.id)}
      <fieldset class="rounded-lg border border-hairline bg-surface px-3 py-2">
        <legend class="px-1 text-xs font-medium text-fg">{group.title}</legend>
        <p class="mb-2 text-[11px] text-muted">{group.description}</p>
        <p class="mb-2 text-[11px] text-fg-2">
          <span class="text-muted">Selected</span>
          {#if selectedModels(group.id).length > 0}
            {#each selectedModels(group.id) as model (model.key)}
              <span class="ml-1 inline-block rounded bg-hover px-1.5 py-0.5">{model.label}</span>
            {/each}
          {:else}
            <span class="ml-1 text-muted">None</span>
          {/if}
        </p>

        <div class="max-h-44 space-y-2 overflow-y-auto pr-1">
          {#if providers.length === 0}
            <p class="text-[11px] text-muted">Add a provider and its models above.</p>
          {:else}
            {#each providers as provider (provider.id)}
              {#if provider.models.length > 0}
                <div>
                  <p class="mb-1 text-[10px] font-medium text-muted">{provider.label}</p>
                  <div class="grid grid-cols-1 gap-x-3 gap-y-1 sm:grid-cols-2">
                    {#each provider.models as model (model.id)}
                      {@const ref = { provider_id: provider.id, model_id: model.id }}
                      <label class="flex min-w-0 items-center gap-2 rounded px-1 py-0.5 text-xs text-fg-2 hover:bg-hover">
                        <input
                          type="checkbox"
                          class="accent-accent"
                          checked={hasReviewModel(draft, group.id, ref)}
                          disabled={saving}
                          onchange={() => toggle(group.id, ref)}
                        />
                        <span class="truncate" title={model.id}>{model.label}</span>
                      </label>
                    {/each}
                  </div>
                </div>
              {/if}
            {/each}
          {/if}

          {#each draft[group.id].filter((ref) => !availableKeys.has(reviewModelKey(ref))) as ref (reviewModelKey(ref))}
            <label class="flex items-center gap-2 rounded px-1 py-0.5 text-xs text-muted">
              <input
                type="checkbox"
                class="accent-accent"
                checked
                disabled={saving}
                onchange={() => toggle(group.id, ref)}
              />
              <span class="truncate">Unavailable: {ref.provider_id} / {ref.model_id}</span>
            </label>
          {/each}
        </div>
      </fieldset>
    {/each}
  </div>

  {#if saveError}
    <p class="mt-2 text-xs text-error" role="alert">Could not save Review model groups. {saveError}</p>
  {/if}

  <div class="mt-3 flex items-center justify-end gap-2">
    <span class="mr-auto text-[11px] text-muted">Changes apply to Review flow only.</span>
    <Button disabled={!dirty || saving} onclick={() => void save()}>
      {saving ? "Saving…" : "Save groups"}
    </Button>
  </div>
</section>
