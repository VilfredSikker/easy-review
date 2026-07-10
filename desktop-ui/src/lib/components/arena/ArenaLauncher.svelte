<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import { openDiffFilePicker } from "$lib/components/AiReviewFilesModal.svelte";
  import { arena, type ArenaStartConfig } from "$lib/stores/arena.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { triageRecommendedPaths } from "$lib/triageSuggestions";
  import ArenaAgentPicker from "$lib/components/arena/ArenaAgentPicker.svelte";
  import type { AiProviderInfo, ReviewerInfo } from "$lib/types";
  import type { ArenaEstimate, ArenaLauncherScope, ArenaScope, ReviewerRef } from "$lib/types/arena";
  import type { LauncherReviewerMode } from "$lib/stores/arena.svelte";
  import {
    formatModelPricePer1k,
    modelLabel,
    pickMostExpensiveModel,
  } from "$lib/arena/estimate";
  import { effortLabel, effortLevelsForModel, modelSupportsEffort } from "$lib/arena/effort";
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
  let costApproved = $state(false);
  let scope = $state<ArenaLauncherScope>("branch");
  let selectedPaths = $state<string[]>([]);
  let title = $state("");
  let loadingProviders = $state(false);

  let estimate = $state<ArenaEstimate | null>(null);
  let estimateLoading = $state(false);
  let estimateError = $state<string | null>(null);
  let estimateSeq = 0;
  /** `provider::model` for round-final arbiter (independent of reviewers). */
  let arbiterKey = $state<string | null>(null);
  let reviewerMode = $state<LauncherReviewerMode>("models");
  let agents = $state<ReviewerInfo[]>([]);
  /** agent_kind → selected provider::model keys */
  let agentSelection = $state<Record<string, Set<string>>>({});
  let effortUseGlobal = $state(true);
  let effortOverride = $state("high");

  const SCOPE_OPTIONS: { id: ArenaLauncherScope; label: string }[] = [
    { id: "branch", label: "Branch" },
    { id: "unstaged", label: "Unstaged" },
    { id: "staged", label: "Staged" },
    { id: "selected", label: "Selected" },
  ];

  const ROUND_OPTIONS = [1, 2, 3, 4, 5] as const;

  function pairKey(p: string, m: string) {
    return `${p}::${m}`;
  }

  function parseKey(key: string): ReviewerRef {
    const [provider_id, model_id] = key.split("::");
    return { provider_id: provider_id ?? "", model_id: model_id ?? "" };
  }

  const launcherMode = $derived(arena.launcherMode);
  const isSingleMode = $derived(launcherMode === "single");

  function toggle(key: string) {
    if (isSingleMode) {
      selected = selected.has(key) ? new Set() : new Set([key]);
    } else {
      const next = new Set(selected);
      if (next.has(key)) next.delete(key);
      else if (next.size < 6) next.add(key);
      selected = next;
    }
    costApproved = false;
    arenaLog("launcher: toggled model", { key, count: selected.size, isSingleMode });
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

  const agentGroups = $derived.by(() => {
    const groups: { agent_kind: string; models: ReviewerRef[] }[] = [];
    for (const [kind, keys] of Object.entries(agentSelection)) {
      if (!keys.size) continue;
      groups.push({
        agent_kind: kind,
        models: [...keys].map((k) => {
          const ref = parseKey(k);
          return { ...ref, agent_kind: kind };
        }),
      });
    }
    return groups;
  });

  const agentModelCount = $derived(
    agentGroups.reduce((n, g) => n + g.models.length, 0),
  );
  const agentArenaCount = $derived(agentGroups.filter((g) => g.models.length >= 2).length);
  const agentSingleCount = $derived(agentGroups.filter((g) => g.models.length === 1).length);

  const isAgentsMode = $derived(reviewerMode === "agents");
  const isArena = $derived(
    isAgentsMode
      ? agentArenaCount > 0 || (agentSingleCount > 0 && agentModelCount > 1)
      : !isSingleMode && picked.length >= 2,
  );
  const minReviewers = $derived(isSingleMode ? 1 : 2);
  const maxReviewers = $derived(isSingleMode ? 1 : 6);

  const effortCapableModelIds = $derived.by(() => {
    if (isAgentsMode) {
      const ids: string[] = [];
      for (const g of agentGroups) {
        for (const m of g.models) {
          if (
            (m.provider_id === "claude" || m.provider_id === "codex") &&
            modelSupportsEffort(m.model_id)
          ) {
            ids.push(m.model_id);
          }
        }
      }
      return ids;
    }
    return picked
      .filter(
        (reviewer) =>
          (reviewer.provider_id === "claude" || reviewer.provider_id === "codex") &&
          modelSupportsEffort(reviewer.model_id),
      )
      .map((reviewer) => reviewer.model_id);
  });

  const usesEffortCapableModels = $derived(effortCapableModelIds.length > 0);

  const effortLevelsForRun = $derived.by(() => {
    if (effortCapableModelIds.length === 0) return [] as readonly string[];
    let levels = [...effortLevelsForModel(effortCapableModelIds[0]!)];
    for (const id of effortCapableModelIds.slice(1)) {
      const next = effortLevelsForModel(id);
      levels = levels.filter((l) => next.includes(l));
    }
    return levels;
  });

  const globalEffortLabel = $derived(
    app.snapshot?.active_ai_effort
      ? effortLabel(app.snapshot.active_ai_effort)
      : "CLI default",
  );

  function arenaRunEffort(): string | undefined {
    if (!usesEffortCapableModels || effortUseGlobal) return undefined;
    return effortOverride;
  }
  const exceedsCostLimit = $derived(
    estimate != null && estimate.cost_usd > estimate.cost_limit_usd,
  );
  const noDiff = $derived(estimate != null && estimate.diff_bytes === 0);

  /** PR tabs and read-only branch views have no working-tree unstaged/staged diff. */
  const arenaDiffReadOnly = $derived(
    app.snapshot?.pr != null ||
      (app.snapshot?.local_branch != null && !app.snapshot?.local_branch_checked_out),
  );

  const scopeOptions = $derived(
    arenaDiffReadOnly
      ? SCOPE_OPTIONS.filter((o) => o.id === "branch" || o.id === "selected")
      : SCOPE_OPTIONS,
  );

  const canStart = $derived(
    (isAgentsMode
      ? agentModelCount >= 1
      : picked.length >= minReviewers && picked.length <= maxReviewers) &&
      !arena.loading &&
      !noDiff &&
      (scope !== "selected" || selectedPaths.length > 0) &&
      (!exceedsCostLimit || costApproved),
  );

  const selectedScopeLabel = $derived(
    selectedPaths.length > 0
      ? `Selected (${selectedPaths.length})`
      : "Selected files…",
  );

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

  const gitPoolLabel = $derived(
    app.snapshot?.mode === "unstaged"
      ? "unstaged"
      : app.snapshot?.mode === "staged"
        ? "staged"
        : app.snapshot?.mode === "pr"
          ? "PR Diff"
          : "branch",
  );

  const scopeHint = $derived(
    scope === "branch"
      ? "Every file on this branch vs base"
      : scope === "unstaged"
        ? "Working tree changes not yet staged"
        : scope === "staged"
          ? "Staged changes ready to commit"
          : selectedPaths.length > 0
            ? `${selectedPaths.length} file${selectedPaths.length === 1 ? "" : "s"} · from ${gitPoolLabel} view`
            : `Pick files from the current ${gitPoolLabel} view`,
  );

  const arbiterRef = $derived(
    arbiterKey ? parseKey(arbiterKey) : null,
  );

  const arbiterLabel = $derived(
    arbiterRef && providers.length ? modelLabel(providers, arbiterRef) : "—",
  );

  const roundsHint = $derived(
    rounds === 1
      ? "Propose only — no cross-check or arbiter"
      : rounds === 2
        ? "Propose + 1 cross-check, then arbiter"
        : rounds === 3
          ? "Propose + 2 cross-checks, then arbiter"
          : `${rounds} reviewer rounds + arbiter`,
  );

  const footerEstimate = $derived(
    estimateLoading
      ? "Estimating…"
      : estimateError
        ? "Estimate unavailable"
        : estimate
          ? noDiff
            ? "No diff in this scope — try Unstaged, Staged, or file selection"
            : `~${estimate.latency_sec}s · ${(estimate.diff_bytes / 1024).toFixed(1)} KB · est. $${estimate.cost_usd.toFixed(2)}`
          : "—",
  );

  function startScopePayload(): { scope: ArenaScope; files?: string[] } {
    if (scope === "selected") {
      return { scope: scopeFromMode(app.snapshot?.mode), files: selectedPaths };
    }
    return { scope };
  }

  function openFilePicker() {
    openDiffFilePicker({
      initialSelected: selectedPaths,
      onConfirm: (paths) => {
        selectedPaths = paths;
        scope = "selected";
        costApproved = false;
        arenaLog("launcher: files selected", { count: paths.length });
      },
    });
  }

  /** Files Triage flagged for review — a one-click scope for the current diff. */
  const triagePaths = $derived(triageRecommendedPaths(app.snapshot?.ai.triage));

  const triageSelected = $derived(
    scope === "selected" &&
      triagePaths.length > 0 &&
      selectedPaths.length === triagePaths.length &&
      triagePaths.every((p) => selectedPaths.includes(p)),
  );

  function useTriageFiles() {
    if (triagePaths.length === 0) return;
    selectedPaths = [...triagePaths];
    scope = "selected";
    costApproved = false;
    arenaLog("launcher: used triage files", { count: triagePaths.length });
  }

  function selectScope(next: ArenaLauncherScope) {
    if (arenaDiffReadOnly && (next === "unstaged" || next === "staged")) {
      return;
    }
    if (next === scope && next === "selected") {
      openFilePicker();
      return;
    }
    scope = next;
    costApproved = false;
    arenaLog("launcher: scope", next);
    if (next === "selected" && selectedPaths.length === 0) {
      openFilePicker();
    }
  }

  function toggleAgent(kind: string) {
    const next = { ...agentSelection };
    if (next[kind]?.size) {
      delete next[kind];
    } else {
      const def = providers.flatMap((p) =>
        p.models.map((m) => pairKey(p.id, m.id)),
      );
      const first = def[0];
      if (first) next[kind] = new Set([first]);
    }
    agentSelection = next;
    costApproved = false;
  }

  function toggleAgentModel(kind: string, key: string) {
    const cur = new Set(agentSelection[kind] ?? []);
    if (isSingleMode) {
      agentSelection = { ...agentSelection, [kind]: cur.has(key) ? new Set() : new Set([key]) };
    } else if (cur.has(key)) {
      cur.delete(key);
      agentSelection = { ...agentSelection, [kind]: cur };
    } else {
      cur.add(key);
      agentSelection = { ...agentSelection, [kind]: cur };
    }
    costApproved = false;
  }

  async function refreshEstimate() {
    if (!open) return;
    if (isAgentsMode) {
      if (agentModelCount < 1) {
        estimate = null;
        return;
      }
    } else if (picked.length < minReviewers) {
      estimate = null;
      estimateError = null;
      estimateLoading = false;
      return;
    }
    const seq = ++estimateSeq;
    estimateLoading = true;
    estimateError = null;
    const { scope: gitScope, files } = startScopePayload();
    try {
      const result = isAgentsMode
        ? await invoke<ArenaEstimate>("arena_estimate_batch", {
            req: {
              scope: gitScope,
              files: scope === "selected" ? files : undefined,
              rounds: Math.min(5, Math.max(1, Number(rounds))),
              arbiter: rounds >= 2 ? arbiterRef ?? undefined : undefined,
              groups: agentGroups.map((g) => ({
                agent_kind: g.agent_kind,
                models: g.models,
              })),
            },
          })
        : await invoke<ArenaEstimate>("arena_estimate", {
            req: {
              reviewers: picked,
              scope: gitScope,
              files: scope === "selected" ? files : undefined,
              rounds: isArena ? Math.min(5, Math.max(1, Number(rounds))) : 1,
              arbiter: isArena && rounds >= 2 ? arbiterRef ?? undefined : undefined,
            },
          });
      if (seq !== estimateSeq) return;
      estimate = result;
      costApproved = false;
      arenaLog("launcher: estimate", result);
    } catch (e) {
      if (seq !== estimateSeq) return;
      estimate = null;
      estimateError = e instanceof Error ? e.message : String(e);
      arenaError("launcher: arena_estimate failed", e);
    } finally {
      if (seq === estimateSeq) estimateLoading = false;
    }
  }

  $effect(() => {
    if (!open) return;
    if (arenaDiffReadOnly && (scope === "unstaged" || scope === "staged")) {
      scope = "branch";
    }
  });

  $effect(() => {
    if (!open) return;
    arenaLog("launcher: opened");
    scope =
      app.snapshot?.mode === "unstaged"
        ? "unstaged"
        : app.snapshot?.mode === "staged"
          ? "staged"
          : "branch";
    selectedPaths = [];
    costApproved = false;
    if (preset.length) {
      const keys = new Set(
        preset
          .slice(0, isSingleMode ? 1 : 6)
          .map((r) => pairKey(r.provider_id, r.model_id)),
      );
      selected = keys;
      rounds = isSingleMode || preset.length < 2 ? 1 : 3;
      arenaLog("launcher: applied preset", { reviewers: preset.length, rounds, isSingleMode });
    } else if (isSingleMode && selected.size > 1) {
      const first = [...selected][0];
      selected = first ? new Set([first]) : new Set();
    }
  });

  $effect(() => {
    if (!open) return;
    const _deps = [
      scope,
      selectedPaths,
      picked.length,
      rounds,
      isArena,
      arbiterKey,
      reviewerMode,
      agentModelCount,
    ];
    void _deps;
    const t = setTimeout(() => void refreshEstimate(), 300);
    return () => clearTimeout(t);
  });

  $effect(() => {
    if (!open) return;
    loadingProviders = true;
    arenaLog("launcher: loading providers");
    Promise.all([
      invoke<AiProviderInfo[]>("list_ai_providers"),
      invoke<ReviewerInfo[]>("list_ai_reviewers"),
    ])
      .then(([list, reviewerList]) => {
        providers = list;
        agents = reviewerList;
        arenaLog("launcher: providers loaded", { count: list.length });
        if (selected.size === 0) {
          const pre: string[] = [];
          for (const p of [...list].sort((a, b) => a.label.localeCompare(b.label))) {
            const m = p.models.find((x) => x.is_selected) ?? p.models[0];
            if (m) pre.push(pairKey(p.id, m.id));
          }
          if (pre.length) {
            const cap = arena.launcherMode === "single" ? 1 : 3;
            selected = new Set(pre.slice(0, cap));
          }
          arenaLog("launcher: default selection", { keys: [...selected] });
        }
        if (!arbiterKey) {
          const def = pickMostExpensiveModel(list);
          if (def) arbiterKey = pairKey(def.provider_id, def.model_id);
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
      costApproved,
      exceedsCostLimit,
    });
    if (!canStart) {
      arenaWarn("launcher: start blocked", {
        reason:
          exceedsCostLimit && !costApproved
            ? "cost not approved"
            : scope === "selected" && selectedPaths.length === 0
              ? "no files selected"
              : picked.length < minReviewers
                ? isSingleMode
                  ? "pick one model"
                  : "need at least 2 reviewers"
                : arena.loading
                  ? "already starting"
                  : "unknown",
      });
      return;
    }

    const { scope: gitScope, files } = startScopePayload();
    const config: ArenaStartConfig = isAgentsMode
      ? {
          mode: "agents",
          agent_groups: agentGroups,
          scope: gitScope,
          files,
          rounds: Math.min(5, Math.max(1, Number(rounds))),
          arbiter: rounds >= 2 ? arbiterRef ?? undefined : undefined,
          confirm: exceedsCostLimit && costApproved,
          effort: arenaRunEffort(),
        }
      : {
          mode: "models",
          title: title.trim() || suggestedTitle,
          reviewers: picked,
          scope: gitScope,
          files,
          rounds: isArena ? Math.min(5, Math.max(1, Number(rounds))) : 1,
          arbiter: isArena && rounds >= 2 ? arbiterRef ?? undefined : undefined,
          confirm: exceedsCostLimit && costApproved,
          effort: arenaRunEffort(),
        };
    arenaLog("launcher: dispatching startRun", config);
    void arena.startRun(config).catch((e) => {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("exceeds limit")) {
        costApproved = false;
        app.showToast(
          "error",
          `Estimated cost exceeds $${estimate?.cost_limit_usd.toFixed(0) ?? "25"} — check the approval box after the estimate updates`,
        );
      }
    });
  }
</script>

<ModalShell
  {open}
  ariaLabel={isSingleMode ? "Start single AI review" : "Start AI Review Arena"}
  {onClose}
  backdropClass="fixed inset-0 z-[250] bg-[color-mix(in_srgb,var(--arena-bg-app)_66%,transparent)]"
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
        <h2 class="text-[14px] font-semibold text-[var(--arena-fg)]">
          {isSingleMode ? "Single AI review" : "AI Review Arena"}
        </h2>
        <p class="text-[11px] text-[var(--arena-fg-subtle)]">
          {#if isSingleMode}
            Pick one model · 1 round
          {:else if isArena}
            {picked.length} reviewers · {rounds} rounds
          {:else}
            Pick at least 2 models for arena
          {/if}
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
      <div class="grid grid-cols-2 gap-2">
        {#each [
          { id: "models" as const, label: "General models", desc: "Same prompt across frontier LLMs" },
          { id: "agents" as const, label: "Specialized agents", desc: "A different agent for each lens" },
        ] as opt (opt.id)}
          <button
            type="button"
            onclick={() => {
              reviewerMode = opt.id;
              costApproved = false;
            }}
            class="rounded-lg border px-3 py-2.5 text-left transition-colors
              {reviewerMode === opt.id
              ? 'border-[var(--arena-periwinkle)] bg-[var(--arena-bg-2)]'
              : 'border-[var(--arena-border)] bg-[var(--arena-bg-0)]'}"
          >
            <p class="text-[12px] font-semibold text-[var(--arena-fg)]">{opt.label}</p>
            <p class="text-[10px] text-[var(--arena-fg-subtle)]">{opt.desc}</p>
          </button>
        {/each}
      </div>

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
          {isAgentsMode ? "Choose agents" : "Choose models"}
        </legend>
        {#if loadingProviders}
          <p class="text-[12px] text-[var(--arena-fg-muted)]">Loading providers…</p>
        {:else if isAgentsMode}
          <ArenaAgentPicker
            {agents}
            {providers}
            selection={agentSelection}
            {isSingleMode}
            onToggleAgent={toggleAgent}
            onToggleModel={toggleAgentModel}
          />
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
            {#each scopeOptions as opt (opt.id)}
              {@const label = opt.id === "selected" ? selectedScopeLabel : opt.label}
              <button
                type="button"
                aria-pressed={scope === opt.id}
                title={arenaDiffReadOnly && opt.id === "branch"
                  ? "Branch diff for this PR or remote branch view"
                  : undefined}
                onclick={() => selectScope(opt.id)}
                class="h-7 min-w-[4.5rem] rounded px-2.5 text-[11px] font-medium transition-colors
                  {scope === opt.id
                  ? 'bg-[var(--arena-bg-3)] text-[var(--arena-fg)] shadow-sm'
                  : 'text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)]'}"
              >
                {label}
              </button>
            {/each}
          </div>
        </div>

        {#if triagePaths.length > 0}
          <button
            type="button"
            aria-pressed={triageSelected}
            onclick={useTriageFiles}
            class="flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left text-[11px] transition-colors
              {triageSelected
              ? 'border-[var(--arena-periwinkle)] bg-[var(--arena-bg-2)]'
              : 'border-[var(--arena-border)] bg-[var(--arena-bg-0)] hover:bg-[var(--arena-bg-2)]'}"
          >
            <span class="text-[var(--arena-info)]">◎</span>
            <span class="font-medium text-[var(--arena-fg)]">
              Review {triagePaths.length} triage-recommended file{triagePaths.length === 1 ? "" : "s"}
            </span>
            <span class="ml-auto text-[var(--arena-fg-subtle)]">from Triage</span>
          </button>
        {/if}

        {#if isArena || (isAgentsMode && agentArenaCount > 0)}
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
                    costApproved = false;
                    arenaLog("launcher: rounds", n);
                  }}
                  class="h-6 w-7 rounded text-[11px] font-semibold mono transition-colors
                    {rounds === n
                    ? 'bg-[var(--arena-periwinkle)] text-[var(--arena-bg-0)]'
                    : 'text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)]'}"
                >
                  {n}
                </button>
              {/each}
            </div>
          </div>

          {#if rounds >= 2}
            <div
              class="grid grid-cols-[minmax(0,7.5rem)_1fr] items-center gap-3 rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-3 py-2.5"
            >
              <div class="min-w-0">
                <p class="text-[11px] font-medium text-[var(--arena-fg)]">Arbiter</p>
                <p class="text-[10px] leading-snug text-[var(--arena-fg-subtle)]">
                  Final verdicts after round {rounds} — can differ from reviewers
                </p>
              </div>
              <div class="flex min-w-0 flex-col items-end gap-1">
                <select
                  class="max-w-full rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-2)] px-2 py-1.5 text-[11px] text-[var(--arena-fg)]"
                  value={arbiterKey ?? ""}
                  onchange={(e) => {
                    arbiterKey = e.currentTarget.value || null;
                    costApproved = false;
                  }}
                >
                  {#each sortedProviders as p (p.id)}
                    <optgroup label={p.label}>
                      {#each p.models as m (m.id)}
                        {@const key = pairKey(p.id, m.id)}
                        <option value={key}>{m.label}</option>
                      {/each}
                    </optgroup>
                  {/each}
                </select>
                <button
                  type="button"
                  class="text-[10px] text-[var(--arena-periwinkle)] hover:underline"
                  onclick={() => {
                    const def = pickMostExpensiveModel(providers);
                    if (def) {
                      arbiterKey = pairKey(def.provider_id, def.model_id);
                      costApproved = false;
                    }
                  }}
                >
                  Use most expensive ({arbiterLabel})
                </button>
              </div>
            </div>
          {/if}
        {/if}

        {#if usesEffortCapableModels && effortLevelsForRun.length > 0}
          <div
            class="rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-3 py-2.5 space-y-2"
          >
            <div class="min-w-0">
              <p class="text-[11px] font-medium text-[var(--arena-fg)]">Effort</p>
              <p class="text-[10px] leading-snug text-[var(--arena-fg-subtle)]">
                Claude and Codex reasoning depth for this run
              </p>
            </div>
            <label class="flex items-center gap-2 text-[11px] text-[var(--arena-fg-muted)]">
              <input
                type="checkbox"
                checked={effortUseGlobal}
                onchange={(e) => {
                  effortUseGlobal = e.currentTarget.checked;
                  costApproved = false;
                }}
                class="rounded border-[var(--arena-border)]"
              />
              Use global ({globalEffortLabel})
            </label>
            {#if !effortUseGlobal}
              <div
                class="inline-flex flex-wrap gap-1 rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-2)] p-0.5"
                role="group"
                aria-label="Arena effort override"
              >
                {#each effortLevelsForRun as level (level)}
                  <button
                    type="button"
                    aria-pressed={effortOverride === level}
                    onclick={() => {
                      effortOverride = level;
                      costApproved = false;
                    }}
                    class="rounded px-2 py-1 text-[11px] font-medium transition-colors
                      {effortOverride === level
                      ? 'bg-[var(--arena-periwinkle)] text-[var(--arena-bg-0)]'
                      : 'text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)]'}"
                  >
                    {effortLabel(level)}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <footer
      class="flex shrink-0 flex-col gap-2 border-t border-[var(--arena-border)] px-5 py-3"
    >
      <div class="flex items-center justify-between gap-3">
        <p class="text-[11px] text-[var(--arena-fg-subtle)]" title={estimateError ?? undefined}>
          {footerEstimate}
          {#if exceedsCostLimit && estimate}
            <span class="text-[var(--arena-warn)]">
              · over ${estimate.cost_limit_usd.toFixed(0)} limit
            </span>
          {/if}
        </p>
        <div class="flex shrink-0 items-center gap-2">
          {#if exceedsCostLimit && estimate}
            <label
              class="flex max-w-[220px] cursor-pointer items-start gap-2 rounded-md border border-[var(--arena-warn)]/40 bg-[color-mix(in_srgb,var(--arena-warn)_8%,transparent)] px-2.5 py-1.5"
            >
              <input
                type="checkbox"
                class="mt-0.5 h-3.5 w-3.5 shrink-0 accent-[var(--arena-periwinkle)]"
                checked={costApproved}
                onchange={(e) => {
                  costApproved = e.currentTarget.checked;
                }}
              />
              <span class="text-[10px] leading-snug text-[var(--arena-fg-subtle)]">
                Approve ~${estimate.cost_usd.toFixed(2)} est. cost
              </span>
            </label>
          {/if}
          <Button variant="secondary" onclick={onClose}>Cancel</Button>
          <button
            type="button"
            disabled={!canStart || estimateLoading}
            onclick={handleStart}
            class="inline-flex h-9 items-center rounded-md bg-[var(--arena-periwinkle)] px-4 text-[12px] font-semibold text-[var(--arena-bg-0)] disabled:opacity-40"
          >
            {arena.loading
              ? "Starting…"
              : isAgentsMode
                ? `Start (${agentArenaCount} arena · ${agentSingleCount} single)`
                : isArena
                  ? "Start arena"
                  : "Start review"}
          </button>
        </div>
      </div>
    </footer>
  </div>
</ModalShell>

<script lang="ts" module>
  function scopeFromMode(mode: string | undefined): ArenaScope {
    if (mode === "unstaged") return "unstaged";
    if (mode === "staged") return "staged";
    return "branch";
  }
</script>
