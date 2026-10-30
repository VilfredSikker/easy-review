<script lang="ts">
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AiSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { diffNav } from "$lib/stores/diffNav.svelte";
  import { baseName, shortFileName } from "$lib/shortFileName";
  import {
    groupRiskRows,
    riskRows,
    sourceLabel,
    tierCounts,
    tierHeading,
    unreviewedHigh,
    type RiskSettingsWire,
    type RiskTier,
  } from "./risksPanel";
  import PanelActions from "./PanelActions.svelte";

  interface Props {
    ai: AiSnapshot | null;
    currentFile: string | null;
  }

  const { ai, currentFile }: Props = $props();

  const TIER_TEXT: Record<RiskTier, string> = {
    high: "text-risk-high",
    med: "text-risk-med",
    low: "text-risk-low",
    ai: "text-periwinkle",
  };
  const TIER_DOT: Record<RiskTier, string> = {
    high: "bg-risk-high",
    med: "bg-risk-med",
    low: "bg-risk-low",
    ai: "bg-periwinkle",
  };

  // Per-file tier and matching rule ride on the snapshot's files (ADR 0048);
  // the settings snapshot is read only for the rule table and its file counts.
  let settings = $state<RiskSettingsWire | null>(null);
  let loadError = $state<string | null>(null);

  const files = $derived(app.snapshot?.files ?? []);
  const fileKey = $derived(files.map((f) => f.path).join("\n"));

  $effect(() => {
    void fileKey;
    let cancelled = false;
    invoke<{ settings: RiskSettingsWire }>("get_config_hub", {})
      .then((res) => {
        if (cancelled) return;
        settings = res.settings;
        loadError = null;
      })
      .catch((e) => {
        if (!cancelled) loadError = String(e);
      });
    return () => {
      cancelled = true;
    };
  });

  const rows = $derived(riskRows(files, ai?.file_risks ?? []));
  const groups = $derived(groupRiskRows(rows));
  const counts = $derived(tierCounts(rows));
  const toRead = $derived(unreviewedHigh(rows));
  const rules = $derived(settings?.riskRules ?? []);
  const declared = $derived(rules.length > 0);

  async function jump(path: string) {
    const file = app.snapshot?.files.find((f) => f.path === path);
    if (!file) return;
    await app.cmd("select_file", { idx: file.source_index });
    await tick();
    await diffNav.scrollToFile(path);
  }

  function toggleReviewed(path: string, reviewed: boolean) {
    void app.cmd(reviewed ? "unmark_reviewed" : "mark_reviewed", { path });
  }

  function ruleTier(tier: string): RiskTier | null {
    return tier === "high" || tier === "med" || tier === "low" || tier === "ai" ? tier : null;
  }
</script>

<div class="flex items-center justify-between gap-2 text-[12.5px] font-medium text-fg">
  <span>{counts.high} high · {counts.med} med · {counts.low} low</span>
  <span class="text-[11px] font-normal text-fg-3">{declared ? "by rules" : "no rules yet"}</span>
</div>

<p class="m-0 text-[11.5px] leading-normal text-fg-3">
  Each file takes the tier of the most specific rule that matches it. Project rules come first, then yours.
  Files no rule fixes are <span class="text-[10px] font-semibold px-1 rounded-[3px] text-periwinkle bg-periwinkle/14">AI</span>:
  the review rates them from what the code does.
  {#if toRead > 0}<span class="text-risk-high">{toRead} high-risk file{toRead === 1 ? "" : "s"} not reviewed yet.</span>{/if}
</p>

{#if loadError}
  <p class="m-0 text-[11px] text-risk-med">Could not read the risk rules: {loadError}</p>
{/if}

<div class="grid gap-0.5 -mx-1.5">
  {#each groups as group (group.tier)}
    <div class="px-2 pt-2 pb-0.5 text-[10.5px] font-semibold uppercase tracking-[0.06em] {TIER_TEXT[group.tier]}">
      {tierHeading(group.tier)}
    </div>
    {#each group.rows as row (row.path)}
      {@const name = shortFileName(baseName(row.path))}
      {@const here = currentFile === row.path}
      <div class="group relative">
        <button
          type="button"
          onclick={() => jump(row.path)}
          title={row.path}
          class="relative w-full grid gap-[3px] px-2 py-[7px] rounded-[7px] text-left hover:bg-hover
            {here ? 'bg-accent/8' : ''} {row.reviewed ? 'opacity-50' : ''}"
        >
          {#if here}<span class="absolute left-0 top-2 bottom-2 w-0.5 rounded bg-accent"></span>{/if}
          <span class="flex items-center gap-1.5 min-w-0 text-[11px] text-fg-3">
            <i class="w-1.5 h-1.5 rounded-[2px] shrink-0 {TIER_DOT[row.shown]}"></i>
            <span class="truncate text-[12.5px] text-fg">{name.main}{#if name.stamp}<span class="text-fg-3"> {name.stamp}</span>{/if}</span>
            {#if row.reviewed}<span class="ml-auto shrink-0 text-[11px]">reviewed</span>{/if}
          </span>
          {#if row.rule}
            <span class="flex items-center gap-1.5 min-w-0 text-[11px] text-fg-3">
              {#if row.rule.label}<span class="truncate">{row.rule.label}</span>{/if}
              <span class="mono truncate">{row.rule.key}</span>
              <span class="ml-auto shrink-0 text-[10.5px] px-[5px] rounded border border-border">{sourceLabel(row.rule.source)}</span>
            </span>
          {/if}
          {#if row.aiReason}
            <span class="text-[12px] text-periwinkle">✦ {row.aiReason}</span>
          {/if}
        </button>
        <button
          type="button"
          onclick={() => toggleReviewed(row.path, row.reviewed)}
          title={row.reviewed ? "Mark not reviewed" : "Mark reviewed"}
          class="absolute top-1.5 right-1 px-1.5 py-px rounded-[5px] mono text-[10.5px] text-fg-3 bg-surface border border-hairline
            opacity-0 group-hover:opacity-100 focus-visible:opacity-100 hover:text-fg transition-opacity"
        >{row.reviewed ? "Unreview" : "Reviewed"}</button>
      </div>
    {/each}
  {:else}
    <p class="px-1 py-3 text-[12px] text-fg-3">No changed files in this view.</p>
  {/each}
</div>

{#if declared}
  <div class="mt-1 text-[10.5px] font-semibold uppercase tracking-[0.06em] text-fg-3">Rules</div>
  <div class="grid gap-px">
    {#each rules as rule (`${rule.source}:${rule.pattern}`)}
      {@const tier = ruleTier(rule.tier)}
      <div class="flex items-center gap-2 px-0.5 py-1 text-[11.5px] {rule.fileCount === 0 ? 'opacity-50' : ''}">
        <i class="w-1.5 h-1.5 rounded-[2px] shrink-0 {tier ? TIER_DOT[tier] : 'bg-fg-3'}"></i>
        <span class="mono text-[11px] text-fg truncate">{rule.pattern}</span>
        <span class="text-fg-3 truncate">{rule.readable ? (rule.label ?? "") : `unreadable tier "${rule.tier}"`}</span>
        <span class="mono text-fg-3 ml-auto shrink-0">{rule.fileCount}</span>
      </div>
    {/each}
  </div>
{/if}

<PanelActions
  actions={[
    { glyph: "✎", label: "Edit risk rules", title: "Settings → Review", run: () => app.setMainView("settings") },
    {
      glyph: "✦",
      label: "Suggest rules for this repo",
      title: "An agent proposes rules and reports how many files each tier captures",
      run: () => void app.cmd("run_importance_agent"),
    },
  ]}
/>
