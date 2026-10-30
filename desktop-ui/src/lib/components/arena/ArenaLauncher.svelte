<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { openDiffFilePicker } from "$lib/components/AiReviewFilesModal.svelte";
  import { arena, type ArenaStartConfig } from "$lib/stores/arena.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { triageRecommendedPaths } from "$lib/triageSuggestions";
  import type { AiProviderInfo, ReviewerInfo } from "$lib/types";
  import type { ArenaEstimate, ArenaLauncherScope, ArenaScope, ReviewerRef } from "$lib/types/arena";
  import { modelLabel, pickMostExpensiveModel } from "$lib/arena/estimate";
  import { effortLabel, effortLevelsForModel, modelSupportsEffort } from "$lib/arena/effort";
  import {
    agentPickKey,
    lensColorVar,
    modelPickKey,
    picksToReviewers,
    reviewerToPickKey,
    triageRecommends,
  } from "$lib/arena/arenaView";
  import { arenaError, arenaLog, arenaWarn } from "$lib/arena/log";

  interface Props {
    open: boolean;
    onClose: () => void;
    preset?: ReviewerRef[];
  }

  const { open, onClose, preset = [] }: Props = $props();

  const MAX_REVIEWERS = 6;

  let providers = $state<AiProviderInfo[]>([]);
  let agents = $state<ReviewerInfo[]>([]);
  let loadingProviders = $state(false);
  /** Model picks (`m:provider::model`) and lens picks (`a:kind`) in one set. */
  let picks = $state<Set<string>>(new Set());
  /** `provider::model` every lens runs on. */
  let agentModelKey = $state<string | null>(null);
  let rounds = $state(3);
  let costApproved = $state(false);
  let scope = $state<ArenaLauncherScope>("branch");
  let selectedPaths = $state<string[]>([]);
  /** `provider::model` for the arbiter's ruling after the last round. */
  let arbiterKey = $state<string | null>(null);
  let effortUseGlobal = $state(true);
  let effortOverride = $state("high");

  let estimate = $state<ArenaEstimate | null>(null);
  let estimateLoading = $state(false);
  let estimateError = $state<string | null>(null);
  let estimateSeq = 0;

  /**
   * The engine clamps rounds to its own maximum (`effective_arena_rounds`), so
   * the picker takes its limit from the estimate rather than a hardcoded
   * number; 3 is only the pre-load fallback.
   */
  const maxRounds = $derived(estimate?.max_rounds ?? 3);
  const roundOptions = $derived(Array.from({ length: maxRounds }, (_, i) => i + 1));

  const SCOPE_OPTIONS: { id: ArenaLauncherScope; label: string }[] = [
    { id: "branch", label: "Branch" },
    { id: "unstaged", label: "Unstaged" },
    { id: "staged", label: "Staged" },
    { id: "selected", label: "Selected" },
  ];

  const isSingleMode = $derived(arena.launcherMode === "single");
  const minReviewers = $derived(isSingleMode ? 1 : 2);
  const maxReviewers = $derived(isSingleMode ? 1 : MAX_REVIEWERS);

  function refFromKey(key: string): ReviewerRef {
    const [provider_id = "", model_id = ""] = key.split("::");
    return { provider_id, model_id };
  }

  const sortedProviders = $derived(
    [...providers]
      .sort((a, b) => a.label.localeCompare(b.label))
      .map((p) => ({ ...p, models: [...p.models].sort((a, b) => a.label.localeCompare(b.label)) })),
  );
  const modelRows = $derived(
    sortedProviders.flatMap((p) => p.models.map((m) => ({ key: modelPickKey(p.id, m.id), provider: p, model: m }))),
  );
  const agentModel = $derived(agentModelKey ? refFromKey(agentModelKey) : null);
  const reviewers = $derived(picksToReviewers(picks, agentModel));
  const isArena = $derived(!isSingleMode && reviewers.length >= 2);
  const effectiveRounds = $derived(isArena ? Math.min(maxRounds, Math.max(1, rounds)) : 1);
  /** ADR 0024: a multi-round run always ends with an arbiter, a one-round run never does. */
  const arbiterOn = $derived(effectiveRounds >= 2);
  const arbiterRef = $derived(arbiterKey ? refFromKey(arbiterKey) : null);
  const arbiterLabel = $derived(arbiterRef && providers.length ? modelLabel(providers, arbiterRef) : "—");
  const triageExperts = $derived(app.snapshot?.ai.triage?.fresh ? app.snapshot.ai.triage.experts : []);

  function modelInfo(ref: ReviewerRef) {
    return providers.find((p) => p.id === ref.provider_id)?.models.find((m) => m.id === ref.model_id) ?? null;
  }

  function modelName(ref: ReviewerRef | null): string {
    if (!ref) return "—";
    return modelInfo(ref)?.label ?? ref.model_id;
  }

  const effortModels = $derived(
    reviewers
      .filter((r) => (r.provider_id === "claude" || r.provider_id === "codex") && modelSupportsEffort(modelInfo(r)))
      .map((r) => modelInfo(r)),
  );
  const effortLevels = $derived.by(() => {
    const [first, ...rest] = effortModels;
    if (!first) return [] as readonly string[];
    let levels = [...effortLevelsForModel(first)];
    for (const m of rest) {
      const next = effortLevelsForModel(m);
      levels = levels.filter((l) => next.includes(l));
    }
    return levels;
  });
  const globalEffortLabel = $derived(
    app.snapshot?.active_ai_effort ? effortLabel(app.snapshot.active_ai_effort) : "CLI default",
  );

  function runEffort(): string | undefined {
    if (effortModels.length === 0 || effortUseGlobal) return undefined;
    return effortOverride;
  }

  const exceedsCostLimit = $derived(estimate != null && estimate.cost_usd > estimate.cost_limit_usd);
  const noDiff = $derived(estimate != null && estimate.diff_bytes === 0);

  /** PR tabs and read-only branch views have no working-tree unstaged/staged diff. */
  const arenaDiffReadOnly = $derived(
    app.snapshot?.pr != null || (app.snapshot?.local_branch != null && !app.snapshot?.local_branch_checked_out),
  );
  const scopeOptions = $derived(
    arenaDiffReadOnly ? SCOPE_OPTIONS.filter((o) => o.id === "branch" || o.id === "selected") : SCOPE_OPTIONS,
  );

  const canStart = $derived(
    reviewers.length >= minReviewers &&
      reviewers.length <= maxReviewers &&
      !arena.loading &&
      !noDiff &&
      (scope !== "selected" || selectedPaths.length > 0) &&
      (!exceedsCostLimit || costApproved),
  );

  const suggestedTitle = $derived.by(() => {
    if (reviewers.length === 0) return "New review run";
    const labels = reviewers.map((r) =>
      r.agent_kind ? agents.find((a) => a.kind === r.agent_kind)?.label ?? r.agent_kind : modelName(r),
    );
    const body = labels.length <= 3 ? labels.join(" × ") : `${labels.slice(0, 2).join(" × ")} +${labels.length - 2}`;
    return isArena ? `${effectiveRounds} rounds · ${body}` : body;
  });

  const triagePaths = $derived(triageRecommendedPaths(app.snapshot?.ai.triage));
  const triageSelected = $derived(
    scope === "selected" &&
      triagePaths.length > 0 &&
      selectedPaths.length === triagePaths.length &&
      triagePaths.every((p) => selectedPaths.includes(p)),
  );

  const footerEstimate = $derived.by(() => {
    if (estimateLoading) return "Estimating…";
    if (estimateError) return "Estimate unavailable";
    if (noDiff) return "No diff in this scope — try Unstaged, Staged, or selected files";
    const shape = isArena
      ? `${reviewers.length} reviewers × ${effectiveRounds} round${effectiveRounds === 1 ? "" : "s"}`
      : `${reviewers.length} reviewer`;
    if (!estimate) return shape;
    const min = Math.max(1, Math.ceil(estimate.latency_sec / 60));
    return `≈ $${estimate.cost_usd.toFixed(2)} · ~${min} min · ${shape}`;
  });

  function touch() {
    costApproved = false;
  }

  function togglePick(key: string) {
    if (isSingleMode) {
      picks = picks.has(key) ? new Set() : new Set([key]);
    } else if (picks.has(key)) {
      picks = new Set([...picks].filter((k) => k !== key));
    } else if (picks.size < MAX_REVIEWERS) {
      picks = new Set([...picks, key]);
    }
    touch();
    arenaLog("launcher: toggled reviewer", { key, count: picks.size, isSingleMode });
  }

  function setArbiter(on: boolean) {
    rounds = on ? Math.max(2, Math.min(maxRounds, rounds >= 2 ? rounds : 3)) : 1;
    touch();
  }

  function startScopePayload(): { scope: ArenaScope; files?: string[] } {
    if (scope === "selected") return { scope: scopeFromMode(app.snapshot?.mode), files: selectedPaths };
    return { scope };
  }

  function openFilePicker() {
    openDiffFilePicker({
      initialSelected: selectedPaths,
      onConfirm: (paths) => {
        selectedPaths = paths;
        scope = "selected";
        touch();
      },
    });
  }

  function useTriageFiles() {
    if (triagePaths.length === 0) return;
    selectedPaths = [...triagePaths];
    scope = "selected";
    touch();
  }

  function selectScope(next: ArenaLauncherScope) {
    if (arenaDiffReadOnly && (next === "unstaged" || next === "staged")) return;
    // The picker sets the scope itself once files are confirmed.
    if (next === "selected" && (scope === "selected" || selectedPaths.length === 0)) {
      openFilePicker();
      return;
    }
    scope = next;
    touch();
  }

  async function refreshEstimate() {
    if (!open) return;
    if (reviewers.length < minReviewers) {
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
      const result = await invoke<ArenaEstimate>("arena_estimate", {
        req: {
          reviewers,
          scope: gitScope,
          files: scope === "selected" ? files : undefined,
          rounds: effectiveRounds,
          arbiter: arbiterOn ? arbiterRef ?? undefined : undefined,
        },
      });
      if (seq !== estimateSeq) return;
      estimate = result;
      costApproved = false;
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
    if (arenaDiffReadOnly && (scope === "unstaged" || scope === "staged")) scope = "branch";
  });

  $effect(() => {
    if (!open) return;
    const mode = app.snapshot?.mode;
    scope = mode === "unstaged" || mode === "staged" ? mode : "branch";
    selectedPaths = [];
    costApproved = false;
    if (preset.length) {
      picks = new Set(preset.slice(0, isSingleMode ? 1 : MAX_REVIEWERS).map(reviewerToPickKey));
      const lens = preset.find((r) => r.agent_kind);
      if (lens) agentModelKey = `${lens.provider_id}::${lens.model_id}`;
      rounds = isSingleMode || preset.length < 2 ? 1 : 3;
    } else if (isSingleMode && picks.size > 1) {
      const first = [...picks][0];
      picks = first ? new Set([first]) : new Set();
    }
  });

  $effect(() => {
    if (!open) return;
    const deps = [scope, selectedPaths, reviewers, effectiveRounds, arbiterKey];
    void deps;
    const t = setTimeout(() => void refreshEstimate(), 300);
    return () => clearTimeout(t);
  });

  $effect(() => {
    if (!open) return;
    loadingProviders = true;
    Promise.all([invoke<AiProviderInfo[]>("list_ai_providers"), invoke<ReviewerInfo[]>("list_ai_reviewers")])
      .then(([list, reviewerList]) => {
        providers = list;
        agents = reviewerList;
        const sorted = [...list].sort((a, b) => a.label.localeCompare(b.label));
        if (!agentModelKey) {
          const p = sorted.find((x) => x.is_selected) ?? sorted[0];
          const m = p?.models.find((x) => x.is_selected) ?? p?.models[0];
          if (p && m) agentModelKey = `${p.id}::${m.id}`;
        }
        if (picks.size === 0) {
          const pre: string[] = [];
          for (const p of sorted) {
            const m = p.models.find((x) => x.is_selected) ?? p.models[0];
            if (m) pre.push(modelPickKey(p.id, m.id));
          }
          picks = new Set(pre.slice(0, arena.launcherMode === "single" ? 1 : 3));
        }
        if (!arbiterKey) {
          const def = pickMostExpensiveModel(list);
          if (def) arbiterKey = `${def.provider_id}::${def.model_id}`;
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
    if (!canStart) {
      arenaWarn("launcher: start blocked", { reviewers: reviewers.length, scope, costApproved, noDiff });
      return;
    }
    const { scope: gitScope, files } = startScopePayload();
    const soleLens = isSingleMode ? reviewers[0]?.agent_kind : undefined;
    const config: ArenaStartConfig = {
      mode: "models",
      title: suggestedTitle,
      reviewers,
      agent_kind: soleLens,
      scope: gitScope,
      files,
      rounds: effectiveRounds,
      arbiter: arbiterOn ? arbiterRef ?? undefined : undefined,
      confirm: exceedsCostLimit && costApproved,
      effort: runEffort(),
    };
    arenaLog("launcher: dispatching startRun", config);
    void arena.startRun(config).catch((e) => {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("exceeds limit")) {
        costApproved = false;
        app.showToast(
          "error",
          `Estimated cost exceeds $${estimate?.cost_limit_usd.toFixed(0) ?? "25"} — approve it once the estimate updates`,
        );
      }
    });
  }
</script>

{#snippet check(on: boolean)}
  <span class="cb" data-on={on} aria-hidden="true">
    <svg width="9" height="9" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="2"><path d="M2.5 6.5l2.5 2.5 4.5-6" /></svg>
  </span>
{/snippet}

<ModalShell
  {open}
  ariaLabel={isSingleMode ? "New single review" : "New arena"}
  {onClose}
  backdropClass="fixed inset-0 z-[250] bg-[color-mix(in_srgb,var(--color-ground)_55%,transparent)]"
  panelClass="left-1/2 top-12 w-[calc(100%-32px)] max-w-[560px] -translate-x-1/2"
>
  <div class="ar-launch">
    <div class="al-h">
      <b>{isSingleMode ? "New single review" : "New arena"}</b>
      <span class="flex-1"></span>
      <button type="button" class="x" onclick={onClose} aria-label="Close">✕</button>
    </div>

    <div class="al-body">
      <div class="al-sec">Scope</div>
      <div class="seg" role="group" aria-label="Diff scope">
        {#each scopeOptions as opt (opt.id)}
          <button type="button" aria-pressed={scope === opt.id} onclick={() => selectScope(opt.id)}>
            {opt.id === "selected" && selectedPaths.length > 0 ? `Selected (${selectedPaths.length})` : opt.label}
          </button>
        {/each}
      </div>
      {#if triagePaths.length > 0}
        <button type="button" class="link" aria-pressed={triageSelected} onclick={useTriageFiles}>
          ◎ Review the {triagePaths.length} file{triagePaths.length === 1 ? "" : "s"} Triage flagged
        </button>
      {/if}

      <div class="al-sec">
        Reviewers <span class="faint">{reviewers.length} picked · {isSingleMode ? "1" : `2–${MAX_REVIEWERS}`}</span>
      </div>
      {#if loadingProviders && providers.length === 0}
        <p class="faint">Loading providers…</p>
      {:else}
        <div class="al-g">General models</div>
        {#each modelRows as row, i (row.key)}
          {@const on = picks.has(row.key)}
          <button type="button" class="al-rev" role="checkbox" aria-checked={on} onclick={() => togglePick(row.key)}>
            {@render check(on)}
            <i style="background:{lensColorVar(undefined, i)}"></i>
            <span class="n">{row.model.label}</span>
            <span class="faint trunc">{row.model.description || row.provider.label}</span>
            <span class="mono faint right">{row.provider.label}</span>
          </button>
        {/each}
        {#if agents.length > 0}
          <div class="al-g row">
            Specialized agents
            <label class="faint agent-model">
              run on
              <select
                value={agentModelKey ?? ""}
                onchange={(e) => {
                  agentModelKey = e.currentTarget.value || null;
                  touch();
                }}
              >
                {#each sortedProviders as p (p.id)}
                  <optgroup label={p.label}>
                    {#each p.models as m (m.id)}<option value="{p.id}::{m.id}">{m.label}</option>{/each}
                  </optgroup>
                {/each}
              </select>
            </label>
          </div>
          {#each agents as a, i (a.kind)}
            {@const key = agentPickKey(a.kind)}
            {@const on = picks.has(key)}
            <button type="button" class="al-rev" role="checkbox" aria-checked={on} onclick={() => togglePick(key)}>
              {@render check(on)}
              <i style="background:{lensColorVar(a.kind, i)}"></i>
              <span class="n">{a.label}</span>
              <span class="faint trunc">{a.description}</span>
              {#if triageRecommends(a.kind, triageExperts)}<span class="chip">◎ from Triage</span>{/if}
              <span class="mono faint right">{modelName(agentModel)}</span>
            </button>
          {/each}
        {/if}
      {/if}

      {#if !isSingleMode}
        <div class="al-grid">
          <div>
            <div class="al-sec">Rounds</div>
            <div class="seg" role="group" aria-label="Arena rounds">
              {#each roundOptions as n (n)}
                <button
                  type="button"
                  aria-pressed={effectiveRounds === n}
                  disabled={!isArena}
                  onclick={() => {
                    rounds = n;
                    touch();
                  }}>{n}</button
                >
              {/each}
            </div>
          </div>
          <div>
            <div class="al-sec">Arbiter</div>
            <div class="arb">
              <button
                type="button"
                class="toggle"
                role="switch"
                aria-checked={arbiterOn}
                disabled={!isArena}
                title="A run with two or more rounds always ends with an arbiter ruling; one round has none"
                onclick={() => setArbiter(!arbiterOn)}
              >
                <span class="sw"></span>{arbiterOn ? arbiterLabel : "Off · one round"}
              </button>
              {#if arbiterOn}
                <select
                  aria-label="Arbiter model"
                  value={arbiterKey ?? ""}
                  onchange={(e) => {
                    arbiterKey = e.currentTarget.value || null;
                    touch();
                  }}
                >
                  {#each sortedProviders as p (p.id)}
                    <optgroup label={p.label}>
                      {#each p.models as m (m.id)}<option value="{p.id}::{m.id}">{m.label}</option>{/each}
                    </optgroup>
                  {/each}
                </select>
              {/if}
            </div>
          </div>
        </div>
      {/if}

      {#if effortLevels.length > 0}
        <div class="al-sec">Effort</div>
        <div class="effort">
          <button
            type="button"
            class="toggle"
            role="switch"
            aria-checked={effortUseGlobal}
            onclick={() => {
              effortUseGlobal = !effortUseGlobal;
              touch();
            }}
          >
            <span class="sw"></span>Use global ({globalEffortLabel})
          </button>
          {#if !effortUseGlobal}
            <div class="seg" role="group" aria-label="Arena effort override">
              {#each effortLevels as level (level)}
                <button
                  type="button"
                  aria-pressed={effortOverride === level}
                  onclick={() => {
                    effortOverride = level;
                    touch();
                  }}>{effortLabel(level)}</button
                >
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <div class="al-foot">
      <span class="mono faint est" title={estimateError ?? undefined}>
        {footerEstimate}{#if exceedsCostLimit && estimate}<span class="warn"> · over ${estimate.cost_limit_usd.toFixed(0)} limit</span>{/if}
      </span>
      <span class="flex-1"></span>
      {#if exceedsCostLimit && estimate}
        <label class="approve">
          <input type="checkbox" bind:checked={costApproved} />
          Approve ≈ ${estimate.cost_usd.toFixed(2)}
        </label>
      {/if}
      <button type="button" class="btn ghost" onclick={onClose}>Cancel</button>
      <button type="button" class="btn primary" disabled={!canStart || estimateLoading} onclick={handleStart}>
        {#if arena.loading}Starting…{:else if isArena}Start arena{:else}Start review{/if}
      </button>
    </div>
  </div>
</ModalShell>

<script lang="ts" module>
  function scopeFromMode(mode: string | undefined): ArenaScope {
    if (mode === "unstaged") return "unstaged";
    if (mode === "staged") return "staged";
    return "branch";
  }
</script>

<style>
  .ar-launch {
    width: 100%;
    max-height: calc(100vh - 64px);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 12px;
    box-shadow: 0 24px 60px color-mix(in srgb, var(--color-ground) 60%, transparent);
    display: flex;
    flex-direction: column;
    color: var(--color-fg);
    font-size: 12.5px;
  }
  .al-h,
  .al-foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
  }
  .al-h {
    border-bottom: 1px solid var(--color-hairline);
  }
  .al-foot {
    border-top: 1px solid var(--color-hairline);
    flex-wrap: wrap;
  }
  .al-body {
    padding: 4px 16px 16px;
    display: grid;
    gap: 6px;
    overflow-y: auto;
    min-height: 0;
  }
  .al-sec {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-muted);
    margin-top: 12px;
    display: flex;
    gap: 8px;
  }
  .al-sec .faint {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
  }
  .al-g {
    font-size: 11px;
    color: var(--color-muted);
    margin-top: 4px;
  }
  .al-g.row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .agent-model {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .al-rev {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 7px;
    text-align: left;
    min-width: 0;
  }
  .al-rev:hover {
    background: var(--color-card);
  }
  .al-rev i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  .n {
    font-weight: 500;
    white-space: nowrap;
  }
  .trunc {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .right {
    margin-left: auto;
    font-size: 11px;
    white-space: nowrap;
  }
  .faint {
    color: var(--color-muted);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: 16px;
    padding: 0 6px;
    border-radius: 5px;
    font-size: 10px;
    white-space: nowrap;
    color: var(--color-fg-3);
    background: var(--color-card);
  }
  .cb {
    width: 14px;
    height: 14px;
    border-radius: 4px;
    border: 1.5px solid var(--color-border);
    display: inline-grid;
    place-items: center;
    flex: none;
    color: var(--color-surface);
  }
  .cb svg {
    opacity: 0;
  }
  .cb[data-on="true"] {
    background: var(--color-success);
    border-color: var(--color-success);
  }
  .cb[data-on="true"] svg {
    opacity: 1;
  }
  .al-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .arb,
  .effort {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
    margin-top: 6px;
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    border-radius: 7px;
    background: var(--color-card);
    justify-self: start;
    margin-top: 6px;
  }
  .effort .seg {
    margin-top: 0;
  }
  .seg button {
    padding: 2px 9px;
    border-radius: 5px;
    color: var(--color-fg-3);
    font-size: 12px;
    font-weight: 500;
  }
  .seg button:disabled {
    opacity: 0.5;
  }
  .seg button[aria-pressed="true"] {
    background: var(--color-surface);
    color: var(--color-fg);
    box-shadow: 0 0 0 1px var(--color-border);
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--color-fg-3);
  }
  .toggle:disabled {
    opacity: 0.5;
  }
  .sw {
    width: 26px;
    height: 16px;
    border-radius: 8px;
    background: var(--color-hover);
    position: relative;
    transition: background 0.15s;
    flex: none;
  }
  .sw::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--color-fg);
    transition: transform 0.15s;
  }
  .toggle[aria-checked="true"] .sw {
    background: var(--color-accent);
  }
  .toggle[aria-checked="true"] .sw::after {
    transform: translateX(10px);
  }
  select {
    max-width: 180px;
    border-radius: 6px;
    border: 1px solid var(--color-border);
    background: var(--color-card);
    color: var(--color-fg);
    padding: 2px 6px;
    font-size: 11.5px;
  }
  .link {
    justify-self: start;
    font-size: 11.5px;
    color: var(--color-fg-3);
  }
  .link:hover,
  .link[aria-pressed="true"] {
    color: var(--color-info);
  }
  .est {
    font-size: 11.5px;
  }
  .warn {
    color: var(--color-warning);
  }
  .approve {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--color-warning);
  }
  .x {
    color: var(--color-fg-3);
    padding: 2px 6px;
    border-radius: 5px;
  }
  .x:hover {
    background: var(--color-card);
    color: var(--color-fg);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--color-border);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn:disabled {
    opacity: 0.45;
  }
  .btn.primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
    border-color: transparent;
  }
  .btn.ghost {
    border-color: transparent;
    background: none;
    color: var(--color-fg-3);
  }
  .btn.ghost:hover {
    background: var(--color-card);
    color: var(--color-fg);
  }
</style>
