<script lang="ts">
  import { untrack } from "svelte";
  import SettingsButton from "./SettingsButton.svelte";
  import Segmented from "./Segmented.svelte";
  import { INPUT_CLASS } from "./settingsModel";
  import { RISK_TIERS, isRiskTier, tierLabel, type RiskRuleDraft, type RiskRuleSource } from "./riskRules";

  interface Props {
    initial?: Partial<RiskRuleDraft>;
    /** Offer the project/yours choice; an edit keeps the rule in its table. */
    chooseSource?: boolean;
    projectAllowed: boolean;
    submitLabel: string;
    busy?: boolean;
    /** Returns why the draft was refused, or null once it landed. */
    onsubmit: (draft: RiskRuleDraft) => Promise<string | null>;
    oncancel?: () => void;
  }

  const { initial = {}, chooseSource = false, projectAllowed, submitLabel, busy = false, onsubmit, oncancel }: Props =
    $props();

  const start = untrack(() => initial);
  let pattern = $state(start.pattern ?? "");
  let tier = $state<string>(start.tier && isRiskTier(start.tier) ? start.tier : "high");
  let label = $state(start.label ?? "");
  let source = $state<RiskRuleSource>(start.source ?? (untrack(() => projectAllowed) ? "project" : "user"));
  let problem = $state<string | null>(null);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!isRiskTier(tier)) return;
    problem = await onsubmit({ source, pattern, tier, label });
    if (problem === null && !oncancel) {
      pattern = "";
      label = "";
    }
  }
</script>

<form class="grid gap-2 w-full" onsubmit={submit} aria-label={submitLabel}>
  <div class="flex flex-wrap gap-2 items-center">
    <input
      class="{INPUT_CLASS} min-w-[180px]"
      aria-label="Pattern"
      placeholder="Path, glob or file type, e.g. db/** or *.sql"
      bind:value={pattern}
      onkeydown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") oncancel?.();
      }}
    />
    <select class="{INPUT_CLASS} flex-none w-[84px] font-sans" aria-label="Tier" bind:value={tier}>
      {#each RISK_TIERS as t (t)}
        <option value={t}>{tierLabel(t)}</option>
      {/each}
    </select>
    <input
      class="{INPUT_CLASS} font-sans min-w-[140px]"
      aria-label="Label"
      placeholder="Label, e.g. Migrations"
      bind:value={label}
      onkeydown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") oncancel?.();
      }}
    />
  </div>
  <div class="flex flex-wrap gap-2 items-center">
    {#if chooseSource}
      <Segmented
        options={["project", "user"]}
        value={source}
        ariaLabel="Rule source"
        display={(o) => (o === "project" ? "This repo" : "All my repos")}
        disabled={!projectAllowed}
        onchange={(o) => (source = o as RiskRuleSource)}
      />
    {/if}
    {#if problem}
      <span class="text-xs text-risk-high" role="alert">{problem}</span>
    {/if}
    <span class="ml-auto flex gap-2">
      {#if oncancel}
        <SettingsButton variant="ghost" onclick={oncancel}>Cancel</SettingsButton>
      {/if}
      <SettingsButton type="submit" disabled={busy || !pattern.trim()}>{submitLabel}</SettingsButton>
    </span>
  </div>
</form>
