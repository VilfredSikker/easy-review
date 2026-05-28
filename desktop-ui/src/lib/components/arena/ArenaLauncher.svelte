<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import { arena, type ArenaStartConfig } from "$lib/stores/arena.svelte";
  import { app } from "$lib/stores/app.svelte";
  import type { AiProviderInfo } from "$lib/types";
  import type { ReviewerRef } from "$lib/types/arena";
  import { estimateArenaCost } from "$lib/arena/estimate";

  interface Props {
    open: boolean;
    onClose: () => void;
    preset?: ReviewerRef[];
  }

  const { open, onClose, preset = [] }: Props = $props();

  let providers = $state<AiProviderInfo[]>([]);
  let selected = $state<Set<string>>(new Set());
  let rounds = $state(3);
  let costConfirmed = $state(false);
  let scope = $state<"branch" | "unstaged" | "staged">("branch");
  let title = $state("");
  let loadingProviders = $state(false);

  function pairKey(p: string, m: string) {
    return `${p}::${m}`;
  }

  function parseKey(key: string): ReviewerRef {
    const [provider_id, model_id] = key.split("::");
    return { provider_id: provider_id ?? "", model_id: model_id ?? "" };
  }

  function toggle(key: string) {
    const next = new Set(selected);
    if (next.has(key)) next.delete(key);
    else if (next.size < 6) next.add(key);
    selected = next;
  }

  const picked = $derived([...selected].map(parseKey));
  const isArena = $derived(picked.length >= 2);
  const canStart = $derived(picked.length >= 2 && picked.length <= 6 && !arena.loading);

  const suggestedTitle = $derived.by(() => {
    if (picked.length === 0) return "New review run";
    const labels: string[] = [];
    for (const p of providers) {
      for (const m of p.models) {
        if (selected.has(pairKey(p.id, m.id))) labels.push(m.label);
      }
    }
    const body =
      labels.length <= 3
        ? labels.join(" × ")
        : `${labels.slice(0, 2).join(" × ")} +${labels.length - 2}`;
    const prefix = rounds >= 3 ? "Deep" : rounds === 2 ? "Standard" : "Quick";
    return `${prefix} · ${body}`;
  });

  const estimate = $derived(estimateArenaCost(providers, picked, rounds));

  $effect(() => {
    if (!open) return;
    scope =
      app.snapshot?.mode === "unstaged"
        ? "unstaged"
        : app.snapshot?.mode === "staged"
          ? "staged"
          : "branch";
    if (preset.length) {
      const keys = new Set(preset.map((r) => pairKey(r.provider_id, r.model_id)));
      selected = keys;
      rounds = preset.length >= 2 ? 3 : 1;
    }
  });

  $effect(() => {
    if (!open) return;
    loadingProviders = true;
    invoke<AiProviderInfo[]>("list_ai_providers")
      .then((list) => {
        providers = list;
        if (selected.size === 0) {
          const pre: string[] = [];
          for (const p of list) {
            const m = p.models.find((x) => x.is_selected) ?? p.models[0];
            if (m) pre.push(pairKey(p.id, m.id));
          }
          if (pre.length) selected = new Set(pre.slice(0, 3));
        }
      })
      .catch((e) => app.showToast("error", String(e)))
      .finally(() => {
        loadingProviders = false;
      });
  });

  function start() {
    const costNum = parseFloat(estimate.costUsd.replace(/[^0-9.]/g, "")) || 0;
    if (costNum > 25 && !costConfirmed) {
      costConfirmed = true;
      app.showToast(
        "info",
        `Estimated cost ${estimate.costUsd} exceeds $25 — click Start again to confirm`,
      );
      return;
    }
    const config: ArenaStartConfig = {
      title: title.trim() || suggestedTitle,
      reviewers: picked,
      scope,
      rounds: isArena ? Math.min(3, Math.max(1, rounds)) : 1,
      confirm: costNum > 25 || costConfirmed,
    };
    costConfirmed = false;
    void arena.startRun(config);
  }
</script>

<ModalShell {open} ariaLabel="Start AI Review Arena" {onClose} panelClass="max-w-[720px] w-full">
  <div
    class="flex max-h-[90vh] flex-col overflow-hidden rounded-[14px] border border-[var(--arena-border)] bg-[var(--arena-bg-1)] shadow-2xl"
  >
    <header
      class="flex shrink-0 items-center justify-between border-b border-[var(--arena-border)] px-5 py-3"
    >
      <div>
        <h2 class="text-[14px] font-semibold text-[var(--arena-fg)]">AI Review Arena</h2>
        <p class="text-[11px] text-[var(--arena-fg-subtle)]">
          Pick reviewers · {isArena ? `${picked.length} reviewers · ${rounds} rounds` : "Single review"}
        </p>
      </div>
      <button
        type="button"
        class="text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)]"
        onclick={onClose}
        aria-label="Close"
      >✕</button>
    </header>

    <div class="flex-1 overflow-y-auto px-5 py-4 space-y-4">
      <label class="block text-[11px] text-[var(--arena-fg-subtle)]">
        Title
        <input
          bind:value={title}
          placeholder={suggestedTitle}
          class="mt-1 w-full rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-3 py-2 text-[12px] text-[var(--arena-fg)]"
        />
      </label>

      <fieldset>
        <legend class="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]">
          Models
        </legend>
        {#if loadingProviders}
          <p class="text-[12px] text-[var(--arena-fg-muted)]">Loading providers…</p>
        {:else}
          <div class="grid gap-2 sm:grid-cols-2">
            {#each providers as p (p.id)}
              {#each p.models as m (m.id)}
                {@const key = pairKey(p.id, m.id)}
                {@const on = selected.has(key)}
                <button
                  type="button"
                  onclick={() => toggle(key)}
                  class="rounded-lg border px-3 py-2 text-left transition-colors
                    {on
                    ? 'border-[var(--arena-periwinkle)] bg-[var(--arena-bg-3)]'
                    : 'border-[var(--arena-border)] bg-[var(--arena-bg-0)] hover:bg-[var(--arena-bg-2)]'}"
                >
                  <p class="text-[12px] font-medium text-[var(--arena-fg)]">{m.label}</p>
                  <p class="text-[10px] text-[var(--arena-fg-subtle)]">{p.label}</p>
                </button>
              {/each}
            {/each}
          </div>
        {/if}
      </fieldset>

      <div class="flex flex-wrap gap-4">
        <label class="text-[11px] text-[var(--arena-fg-subtle)]">
          Scope
          <select
            bind:value={scope}
            class="mt-1 block rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-2 py-1.5 text-[12px] text-[var(--arena-fg)]"
          >
            <option value="branch">Branch diff</option>
            <option value="unstaged">Unstaged</option>
            <option value="staged">Staged</option>
          </select>
        </label>
        {#if isArena}
          <label class="text-[11px] text-[var(--arena-fg-subtle)]">
            Rounds
            <input
              type="range"
              min="1"
              max="3"
              bind:value={rounds}
              class="mt-2 block w-32"
            />
            <span class="mono text-[12px] text-[var(--arena-fg)]">{rounds}</span>
          </label>
        {/if}
      </div>
    </div>

    <footer
      class="flex shrink-0 items-center justify-between gap-3 border-t border-[var(--arena-border)] px-5 py-3"
    >
      <p class="text-[11px] text-[var(--arena-fg-subtle)]">
        ~{estimate.latencySec}s · est. {estimate.costUsd}
      </p>
      <div class="flex gap-2">
        <Button variant="secondary" onclick={onClose}>Cancel</Button>
        <button
          type="button"
          disabled={!canStart}
          onclick={start}
          class="inline-flex h-9 items-center rounded-md bg-[var(--arena-periwinkle)] px-4 text-[12px] font-semibold text-white disabled:opacity-40"
        >
          {arena.loading ? "Starting…" : isArena ? "Start arena" : "Start review"}
        </button>
      </div>
    </footer>
  </div>
</ModalShell>
