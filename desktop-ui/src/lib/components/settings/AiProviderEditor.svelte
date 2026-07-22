<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { AiModelInfo, AiProviderInfo, GetConfigHubResponse } from "$lib/types";
  import { EFFORT_LEVELS, effortLabel } from "$lib/arena/effort";
  import Button from "$lib/components/ui/Button.svelte";

  interface Props {
    providers: AiProviderInfo[];
    familyOptions?: string[];
    onUpdated: (res: GetConfigHubResponse) => void;
    onError: (msg: string) => void;
  }

  let { providers, familyOptions, onUpdated, onError }: Props = $props();

  const FALLBACK_FAMILY_OPTIONS = ["claude", "codex", "cursor", "opencode"];
  const familyChoices = $derived(
    [""].concat(familyOptions?.length ? familyOptions : FALLBACK_FAMILY_OPTIONS),
  );

  let editingProviderId = $state<string | null>(null);
  let draftId = $state("");
  let draftLabel = $state("");
  let draftCommand = $state("");
  let draftArgs = $state("");
  let draftFamily = $state("");
  let draftModelsCommand = $state("");
  let addingProvider = $state(false);
  let formWarnings = $state<string[]>([]);

  let editingModelKey = $state<string | null>(null); // providerId::modelId
  let modelDraft = $state({
    id: "",
    label: "",
    description: "",
    args: "",
    effortLevels: [] as string[],
  });

  function startEditProvider(p: AiProviderInfo) {
    addingProvider = false;
    editingProviderId = p.id;
    draftId = p.id;
    draftLabel = p.label;
    draftCommand = p.command ?? "";
    draftArgs = p.args ?? "";
    draftFamily = p.family ?? "";
    draftModelsCommand = p.models_command ?? "";
    formWarnings = [];
  }

  function startAddProvider() {
    addingProvider = true;
    editingProviderId = null;
    draftId = "";
    draftLabel = "";
    draftCommand = "";
    draftArgs = "{prompt}";
    draftFamily = "";
    draftModelsCommand = "";
    formWarnings = [];
  }

  function cancelProviderEdit() {
    addingProvider = false;
    editingProviderId = null;
    formWarnings = [];
  }

  function handleUpdated(res: GetConfigHubResponse) {
    formWarnings = res.warnings ?? [];
    onUpdated(res);
  }

  function toggleEffort(level: string) {
    if (modelDraft.effortLevels.includes(level)) {
      modelDraft.effortLevels = modelDraft.effortLevels.filter((l) => l !== level);
    } else {
      modelDraft.effortLevels = [...modelDraft.effortLevels, level];
    }
  }

  async function saveProvider(originalId: string | null) {
    try {
      const res = await invoke<GetConfigHubResponse>("upsert_ai_provider", {
        provider: {
          id: draftId.trim(),
          originalId,
          label: draftLabel.trim() || null,
          command: draftCommand.trim(),
          args: draftArgs,
          family: draftFamily.trim() || null,
          modelsCommand: draftModelsCommand.trim() || null,
        },
      });
      addingProvider = false;
      editingProviderId = null;
      handleUpdated(res);
    } catch (e) {
      onError(String(e));
    }
  }

  async function removeProvider(id: string) {
    try {
      const res = await invoke<GetConfigHubResponse>("delete_ai_provider", {
        providerId: id,
      });
      handleUpdated(res);
    } catch (e) {
      onError(String(e));
    }
  }

  function startEditModel(providerId: string, m: AiModelInfo) {
    editingModelKey = `${providerId}::${m.id}`;
    modelDraft = {
      id: m.id,
      label: m.label,
      description: m.description ?? "",
      args: m.args ?? "",
      effortLevels: [...(m.effort_levels ?? [])],
    };
    formWarnings = [];
  }

  function startAddModel(providerId: string) {
    editingModelKey = `${providerId}::`;
    modelDraft = {
      id: "",
      label: "",
      description: "",
      args: "--model ",
      effortLevels: [],
    };
    formWarnings = [];
  }

  async function saveModel(providerId: string, originalId: string | null) {
    try {
      const res = await invoke<GetConfigHubResponse>("upsert_ai_model", {
        providerId,
        model: {
          id: modelDraft.id.trim(),
          originalId,
          label: modelDraft.label.trim() || null,
          description: modelDraft.description.trim() || null,
          args: modelDraft.args,
          effortLevels: modelDraft.effortLevels,
          costPer1kIn: null,
          costPer1kOut: null,
          avgLatencyMs: null,
        },
      });
      handleUpdated(res);
      editingModelKey = null;
    } catch (e) {
      onError(String(e));
    }
  }

  async function removeModel(providerId: string, modelId: string) {
    try {
      const res = await invoke<GetConfigHubResponse>("delete_ai_model", {
        providerId,
        modelId,
      });
      handleUpdated(res);
    } catch (e) {
      onError(String(e));
    }
  }
</script>

<div class="mt-3 space-y-3 border-t border-hairline pt-3">
  <div class="flex items-center justify-between gap-2">
    <p class="text-xs text-muted">Edit providers and preset models. Discovered models appear after Refresh.</p>
    <Button variant="ghost" onclick={startAddProvider}>Add provider</Button>
  </div>

  {#if formWarnings.length > 0}
    <div class="rounded-md border border-accent-border bg-accent-soft/40 px-3 py-2 text-xs text-fg-2 space-y-1">
      {#each formWarnings as w (w)}
        <p>{w}</p>
      {/each}
    </div>
  {/if}

  {#if addingProvider}
    <div class="rounded-lg border border-border bg-surface px-3 py-3 space-y-2">
      <div class="grid grid-cols-2 gap-2 text-xs">
        <label class="space-y-1">
          <span class="text-muted">Id</span>
          <input class="w-full rounded border border-hairline bg-card px-2 py-1 font-mono" bind:value={draftId} />
        </label>
        <label class="space-y-1">
          <span class="text-muted">Label</span>
          <input class="w-full rounded border border-hairline bg-card px-2 py-1" bind:value={draftLabel} />
        </label>
        <label class="space-y-1">
          <span class="text-muted">Command</span>
          <input class="w-full rounded border border-hairline bg-card px-2 py-1 font-mono" bind:value={draftCommand} />
        </label>
        <label class="space-y-1">
          <span class="text-muted">Family</span>
          <select class="w-full rounded border border-hairline bg-card px-2 py-1" bind:value={draftFamily}>
            {#each familyChoices as opt (opt)}
              <option value={opt}>{opt || "(detect)"}</option>
            {/each}
          </select>
        </label>
        <label class="col-span-2 space-y-1">
          <span class="text-muted">Args</span>
          <input class="w-full rounded border border-hairline bg-card px-2 py-1 font-mono" bind:value={draftArgs} />
        </label>
        <label class="col-span-2 space-y-1">
          <span class="text-muted">models_command</span>
          <input
            class="w-full rounded border border-hairline bg-card px-2 py-1 font-mono"
            bind:value={draftModelsCommand}
            placeholder="agent --list-models"
          />
        </label>
      </div>
      <div class="flex gap-2">
        <Button variant="primary" onclick={() => void saveProvider(null)}>Save</Button>
        <Button variant="ghost" onclick={cancelProviderEdit}>Cancel</Button>
      </div>
    </div>
  {/if}

  {#each providers as p (p.id)}
    <div class="rounded-lg border border-hairline bg-card px-3 py-2 space-y-2">
      {#if editingProviderId === p.id}
        <div class="grid grid-cols-2 gap-2 text-xs">
          <label class="space-y-1">
            <span class="text-muted">Id</span>
            <input class="w-full rounded border border-hairline bg-surface px-2 py-1 font-mono" bind:value={draftId} />
          </label>
          <label class="space-y-1">
            <span class="text-muted">Label</span>
            <input class="w-full rounded border border-hairline bg-surface px-2 py-1" bind:value={draftLabel} />
          </label>
          <label class="space-y-1">
            <span class="text-muted">Command</span>
            <input class="w-full rounded border border-hairline bg-surface px-2 py-1 font-mono" bind:value={draftCommand} />
          </label>
          <label class="space-y-1">
            <span class="text-muted">Family</span>
            <select class="w-full rounded border border-hairline bg-surface px-2 py-1" bind:value={draftFamily}>
              {#each familyChoices as opt (opt)}
                <option value={opt}>{opt || "(detect)"}</option>
              {/each}
            </select>
          </label>
          <label class="col-span-2 space-y-1">
            <span class="text-muted">Args</span>
            <input class="w-full rounded border border-hairline bg-surface px-2 py-1 font-mono" bind:value={draftArgs} />
          </label>
          <label class="col-span-2 space-y-1">
            <span class="text-muted">models_command</span>
            <input class="w-full rounded border border-hairline bg-surface px-2 py-1 font-mono" bind:value={draftModelsCommand} />
          </label>
        </div>
        <div class="flex gap-2">
          <Button variant="primary" onclick={() => void saveProvider(p.id)}>Save</Button>
          <Button variant="ghost" onclick={cancelProviderEdit}>Cancel</Button>
        </div>
      {:else}
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <div class="text-sm text-fg font-medium truncate">{p.label}</div>
            <div class="text-[11px] font-mono text-muted truncate">
              {p.id} · {p.command ?? "?"}{#if p.family} · {p.family}{/if}
            </div>
          </div>
          <div class="flex gap-1 shrink-0">
            <Button variant="ghost" onclick={() => startEditProvider(p)}>Edit</Button>
            <Button variant="ghost" onclick={() => void removeProvider(p.id)}>Delete</Button>
          </div>
        </div>
      {/if}

      <div class="space-y-1">
        <div class="flex items-center justify-between">
          <span class="text-[11px] uppercase tracking-wider text-muted">Models</span>
          <Button variant="ghost" onclick={() => startAddModel(p.id)}>Add model</Button>
        </div>
        {#each p.models as m (m.id)}
          {#if editingModelKey === `${p.id}::${m.id}`}
            <div class="rounded border border-border bg-surface px-2 py-2 space-y-1 text-xs">
              <input class="w-full rounded border border-hairline bg-card px-2 py-1 font-mono" bind:value={modelDraft.id} placeholder="id" />
              <input class="w-full rounded border border-hairline bg-card px-2 py-1" bind:value={modelDraft.label} placeholder="label" />
              <input class="w-full rounded border border-hairline bg-card px-2 py-1 font-mono" bind:value={modelDraft.args} placeholder="args" disabled={m.discovered} />
              <div class="space-y-1">
                <span class="text-muted">Effort levels</span>
                <div class="flex flex-wrap gap-1">
                  {#each EFFORT_LEVELS as level (level)}
                    <button
                      type="button"
                      class="rounded px-2 py-0.5 text-[11px] border transition-colors {modelDraft.effortLevels.includes(level)
                        ? 'bg-accent-soft text-accent border-accent-border'
                        : 'bg-card text-fg-2 border-hairline hover:bg-hover'}"
                      onclick={() => toggleEffort(level)}
                    >
                      {effortLabel(level)}
                    </button>
                  {/each}
                </div>
              </div>
              <div class="flex gap-2">
                <Button variant="primary" onclick={() => void saveModel(p.id, m.id)}>Save</Button>
                <Button variant="ghost" onclick={() => (editingModelKey = null)}>Cancel</Button>
              </div>
            </div>
          {:else}
            <div class="flex items-center justify-between gap-2 text-xs py-0.5">
              <div class="min-w-0 truncate">
                <span class="text-fg">{m.label}</span>
                <span class="font-mono text-muted"> · {m.id}</span>
                {#if m.discovered}<span class="text-accent"> · discovered</span>{/if}
              </div>
              <div class="flex gap-1 shrink-0">
                <Button variant="ghost" onclick={() => startEditModel(p.id, m)}>Edit</Button>
                <Button variant="ghost" onclick={() => void removeModel(p.id, m.id)}>Delete</Button>
              </div>
            </div>
          {/if}
        {/each}
        {#if editingModelKey === `${p.id}::`}
          <div class="rounded border border-border bg-surface px-2 py-2 space-y-1 text-xs">
            <input class="w-full rounded border border-hairline bg-card px-2 py-1 font-mono" bind:value={modelDraft.id} placeholder="id" />
            <input class="w-full rounded border border-hairline bg-card px-2 py-1" bind:value={modelDraft.label} placeholder="label" />
            <input class="w-full rounded border border-hairline bg-card px-2 py-1 font-mono" bind:value={modelDraft.args} placeholder="args" />
            <div class="space-y-1">
              <span class="text-muted">Effort levels</span>
              <div class="flex flex-wrap gap-1">
                {#each EFFORT_LEVELS as level (level)}
                  <button
                    type="button"
                    class="rounded px-2 py-0.5 text-[11px] border transition-colors {modelDraft.effortLevels.includes(level)
                      ? 'bg-accent-soft text-accent border-accent-border'
                      : 'bg-card text-fg-2 border-hairline hover:bg-hover'}"
                    onclick={() => toggleEffort(level)}
                  >
                    {effortLabel(level)}
                  </button>
                {/each}
              </div>
            </div>
            <div class="flex gap-2">
              <Button variant="primary" onclick={() => void saveModel(p.id, null)}>Save</Button>
              <Button variant="ghost" onclick={() => (editingModelKey = null)}>Cancel</Button>
            </div>
          </div>
        {/if}
      </div>
    </div>
  {/each}
</div>
