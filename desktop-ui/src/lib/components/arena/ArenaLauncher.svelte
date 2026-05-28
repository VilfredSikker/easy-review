<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import { arena, type ArenaStartConfig } from "$lib/stores/arena.svelte";
  import { app } from "$lib/stores/app.svelte";
  import type { AiProviderInfo } from "$lib/types";
  import type { ReviewerRef } from "$lib/types/arena";
  import { estimateArenaCost, formatModelPricePer1k } from "$lib/arena/estimate";
  import { arenaError, arenaLog, arenaWarn } from "$lib/arena/log";

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

  const SCOPE_OPTIONS: { id: typeof scope; label: string }[] = [
    { id: "branch", label: "Branch" },
    { id: "unstaged", label: "Unstaged" },
    { id: "staged", label: "Staged" },
  ];

  const ROUND_OPTIONS = [1, 2, 3, 4, 5] as const;

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
    arenaLog("launcher: toggled model", { key, count: next.size });
  }

  const sortedProviders = $derived(
    [...providers]
      .sort((a, b) => a.label.localeCompare(b.label))
      .map((p) => ({
        ...p,
        models: [...p.models].sort((a, b) => a.label.localeCompare(b.label)),
      })),
  );

  const picked = $derived([...selected].map(parseKey));
  const isArena = $derived(picked.length >= 2);
  const canStart = $derived(picked.length >= 2 && picked.length <= 6 && !arena.loading);

  const suggestedTitle = $derived.by(() => {
    if (picked.length === 0) return "New review run";
    const labels: string[] = [];
    for (const p of sortedProviders) {
      for (const m of p.models) {
        if (selected.has(pairKey(p.id, m.id))) labels.push(m.label);
      }
    }
    const body =
      labels.length <= 3
        ? labels.join(" × ")
        : `${labels.slice(0, 2).join(" × ")} +${labels.length - 2}`;
    const prefix =
      rounds >= 4 ? "Deep" : rounds === 3 ? "Standard" : rounds === 2 ? "Quick" : "Light";
    return `${prefix} · ${body}`;
  });

  const estimate = $derived(estimateArenaCost(providers, picked, rounds));

  const scopeHint = $derived(
    scope === "branch"
      ? "Every file on this branch vs base"
      : scope === "unstaged"
        ? "Working tree changes not yet staged"
        : "Staged changes ready to commit",
  );

  const roundsHint = $derived(
    rounds === 1
      ? "Propose only — no cross-check"
      : rounds === 2
        ? "Propose + cross-check"
        : rounds === 3
          ? "Full debate + resolve"
          : `${rounds} rounds (deep)`,
  );

  $effect(() => {
    if (!open) return;
    arenaLog("launcher: opened");
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
      arenaLog("launcher: applied preset", { reviewers: preset.length, rounds });
    }
  });

  $effect(() => {
    if (!open) return;
    loadingProviders = true;
    arenaLog("launcher: loading providers");
    invoke<AiProviderInfo[]>("list_ai_providers")
      .then((list) => {
        providers = list;
        arenaLog("launcher: providers loaded", { count: list.length });
        if (selected.size === 0) {
          const pre: string[] = [];
          for (const p of [...list].sort((a, b) => a.label.localeCompare(b.label))) {
            const m = p.models.find((x) => x.is_selected) ?? p.models[0];
            if (m) pre.push(pairKey(p.id, m.id));
          }
          if (pre.length) selected = new Set(pre.slice(0, 3));
          arenaLog("launcher: default selection", { keys: [...selected] });
        }
      })
      .catch((e) => {
        arenaError("launcher: list_ai_providers failed", e);
        app.showToast("error", String(e));
      })
      .finally(() => {
        loadingProviders = false;
      });
  });

  function handleStart() {
    arenaLog("launcher: Start clicked", {
      canStart,
      picked: picked.length,
      loading: arena.loading,
      scope,
      rounds,
    });
    if (!canStart) {
      arenaWarn("launcher: start blocked", {
        reason:
          picked.length < 2
            ? "need at least 2 reviewers"
            : arena.loading
              ? "already starting"
              : "unknown",
      });
      return;
    }

    const costNum = parseFloat(estimate.costUsd.replace(/[^0-9.]/g, "")) || 0;
    if (costNum > 25 && !costConfirmed) {
      costConfirmed = true;
      arenaLog("launcher: cost confirm required", { costUsd: estimate.costUsd });
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
      rounds: isArena ? Math.min(5, Math.max(1, Number(rounds))) : 1,
      confirm: costNum > 25 || costConfirmed,
    };
    costConfirmed = false;
    arenaLog("launcher: dispatching startRun", config);
    void arena.startRun(config);
  }
</script>

<ModalShell
  {open}
  ariaLabel="Start AI Review Arena"
  {onClose}
  backdropClass="fixed inset-0 z-[250] bg-[rgba(8,12,20,0.66)]"
  backdropStyle="backdrop-filter: blur(4px); -webkit-backdrop-filter: blur(4px);"
  panelClass="left-1/2 top-1/2 w-full max-w-[720px] -translate-x-1/2 -translate-y-1/2"
>
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
          <div class="space-y-4">
            {#each sortedProviders as p (p.id)}
              <div>
                <p
                  class="mb-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]"
                >
                  {p.label}
                </p>
                <div class="grid gap-2 sm:grid-cols-2">
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
                      {#if m.description}
                        <p class="text-[10px] text-[var(--arena-fg-subtle)]">{m.description}</p>
                      {/if}
                      <p class="font-mono text-[10px] text-[var(--arena-fg-faint)]">
                        {p.label} · {formatModelPricePer1k(m)}
                      </p>
                    </button>
                  {/each}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </fieldset>

      <div class="space-y-2">
        <p
          class="text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]"
        >
          Run settings
        </p>

        <div
          class="grid grid-cols-[minmax(0,7.5rem)_1fr] items-center gap-3 rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-3 py-2.5"
        >
          <div class="min-w-0">
            <p class="text-[11px] font-medium text-[var(--arena-fg)]">Scope</p>
            <p class="text-[10px] leading-snug text-[var(--arena-fg-subtle)]">{scopeHint}</p>
          </div>
          <div
            class="inline-flex flex-wrap justify-end gap-1 rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-2)] p-0.5"
            role="group"
            aria-label="Diff scope"
          >
            {#each SCOPE_OPTIONS as opt (opt.id)}
              <button
                type="button"
                aria-pressed={scope === opt.id}
                onclick={() => {
                  scope = opt.id;
                  arenaLog("launcher: scope", opt.id);
                }}
                class="h-7 min-w-[4.5rem] rounded px-2.5 text-[11px] font-medium transition-colors
                  {scope === opt.id
                  ? 'bg-[var(--arena-bg-3)] text-[var(--arena-fg)] shadow-sm'
                  : 'text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)]'}"
              >
                {opt.label}
              </button>
            {/each}
          </div>
        </div>

        {#if isArena}
          <div
            class="grid grid-cols-[minmax(0,7.5rem)_1fr] items-center gap-3 rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-3 py-2.5"
          >
            <div class="min-w-0">
              <p class="text-[11px] font-medium text-[var(--arena-fg)]">Rounds</p>
              <p class="text-[10px] leading-snug text-[var(--arena-fg-subtle)]">{roundsHint}</p>
            </div>
            <div
              class="inline-flex justify-end gap-0.5 rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-2)] p-0.5"
              role="group"
              aria-label="Arena rounds"
            >
              {#each ROUND_OPTIONS as n (n)}
                <button
                  type="button"
                  aria-pressed={rounds === n}
                  onclick={() => {
                    rounds = n;
                    arenaLog("launcher: rounds", n);
                  }}
                  class="h-6 w-7 rounded text-[11px] font-semibold mono transition-colors
                    {rounds === n
                    ? 'bg-[var(--arena-periwinkle)] text-[#0e1420]'
                    : 'text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)]'}"
                >
                  {n}
                </button>
              {/each}
            </div>
          </div>
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
          onclick={handleStart}
          class="inline-flex h-9 items-center rounded-md bg-[var(--arena-periwinkle)] px-4 text-[12px] font-semibold text-white disabled:opacity-40"
        >
          {arena.loading ? "Starting…" : isArena ? "Start arena" : "Start review"}
        </button>
      </div>
    </footer>
  </div>
</ModalShell>
