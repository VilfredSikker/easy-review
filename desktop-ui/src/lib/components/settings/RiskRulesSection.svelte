<script lang="ts">
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import SettingsButton from "./SettingsButton.svelte";
  import RiskTierPill from "./RiskTierPill.svelte";
  import RiskRuleForm from "./RiskRuleForm.svelte";
  import {
    claimedBy,
    draftProblem,
    fileWindow,
    isRiskTier,
    removeRuleEdit,
    ruleLanded,
    setRuleEdit,
    tierLabel,
    type RiskFileRow,
    type RiskRuleDraft,
    type RiskRuleRow,
    type RiskTier,
  } from "./riskRules";

  interface Props {
    repo: string;
    rules: RiskRuleRow[];
    defaultTier: RiskTier;
    files: RiskFileRow[];
    /** Send one `risk.rule` edit; resolves to the rules after the write, or
     *  null when the write itself failed (already reported). */
    onedit: (edit: string) => Promise<RiskRuleRow[] | null>;
    onpropose: () => void;
  }

  const { repo, rules, defaultTier, files, onedit, onpropose }: Props = $props();

  const GRID = "grid grid-cols-[minmax(0,1fr)_auto_auto] min-[880px]:grid-cols-[minmax(0,1.6fr)_64px_minmax(0,1fr)_64px_40px_44px] gap-x-2.5 items-center";

  let editing = $state<string | null>(null);
  let adding = $state(false);
  let busy = $state(false);
  let showFiles = $state(false);

  const rowId = (r: Pick<RiskRuleRow, "source" | "pattern">) => `${r.source}\u0000${r.pattern}`;
  const fileRows = $derived(fileWindow(files));
  const keywords = $derived(["Risk rules", "Propose rules", "Changed files", ...rules.flatMap((r) => [r.pattern, r.label ?? ""])]);

  // The engine refuses a bad edit silently (the patch wire carries no reason),
  // so a write is checked by reading the rule back.
  async function save(draft: RiskRuleDraft, replaces?: string): Promise<string | null> {
    const problem = draftProblem(draft, repo, rules, replaces);
    if (problem) return problem;
    busy = true;
    try {
      const after = await onedit(setRuleEdit(draft, repo, replaces));
      if (after === null) return "Could not save the rule.";
      if (!ruleLanded(after, draft)) return `${draft.pattern.trim()} is not a pattern er can match.`;
      editing = null;
      return null;
    } finally {
      busy = false;
    }
  }

  async function remove(rule: RiskRuleRow) {
    busy = true;
    try {
      await onedit(removeRuleEdit(rule, repo));
    } finally {
      busy = false;
    }
  }
</script>

<SettingsSection title="Risk rules" {keywords}>
  {#snippet aside()}<span class="font-mono" title="The repo these project rules belong to">{repo || "no repo open"}</span>{/snippet}
  {#if rules.length > 0}
    <div
      class="{GRID} px-3.5 py-2 border-b border-line text-[10.5px] font-semibold uppercase tracking-[.06em] text-muted max-[879px]:hidden"
      aria-hidden="true"
    >
      <span>Pattern</span><span>Tier</span><span>Label</span><span>Source</span><span>Files</span><span></span>
    </div>
    <ul aria-label="Risk rules">
      {#each rules as rule (rowId(rule))}
        <li class="px-3.5 py-2 border-t border-line first:border-t-0 text-[12.5px]">
          {#if editing === rowId(rule)}
            <RiskRuleForm
              initial={{ ...rule, tier: isRiskTier(rule.tier) ? rule.tier : "high", label: rule.label ?? "" }}
              projectAllowed={!!repo}
              submitLabel="Save rule"
              {busy}
              onsubmit={(draft) => save({ ...draft, source: rule.source }, rule.pattern)}
              oncancel={() => (editing = null)}
            />
          {:else}
            <div class="{GRID} {rule.fileCount === 0 ? 'opacity-60' : ''}">
              <span class="font-mono text-[11.5px] text-fg truncate" title={rule.pattern}>{rule.pattern}</span>
              <RiskTierPill tier={rule.tier} readable={rule.readable} />
              <span class="truncate text-fg-2 max-[879px]:hidden">{rule.label ?? ""}</span>
              <span class="max-[879px]:hidden">
                <span
                  class="text-[10.5px] px-[5px] rounded border border-border text-fg-3 whitespace-nowrap"
                  title={rule.source === "project" ? "This repo's rules, searched first" : "Your rules, applied in every repo"}
                >{rule.source === "project" ? "project" : "yours"}</span>
              </span>
              <span class="font-mono text-muted max-[879px]:hidden" title="Changed files this rule claims">{rule.fileCount}</span>
              <span class="flex justify-end gap-0.5">
                <button
                  type="button"
                  class="w-[20px] h-[20px] grid place-items-center rounded text-fg-3 hover:bg-hover hover:text-fg text-[11px]"
                  aria-label="Edit {rule.pattern}"
                  title="Edit"
                  disabled={busy}
                  onclick={() => {
                    adding = false;
                    editing = rowId(rule);
                  }}
                >✎</button>
                <button
                  type="button"
                  class="w-[20px] h-[20px] grid place-items-center rounded text-fg-3 hover:bg-hover hover:text-risk-high text-[14px] leading-none"
                  aria-label="Remove {rule.pattern}"
                  title="Remove"
                  disabled={busy}
                  onclick={() => void remove(rule)}
                >×</button>
              </span>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <SettingsRow
      always
      label="No rules declared"
      description="Every file reads as {tierLabel(defaultTier)} until a rule claims it. Add one, or let an agent propose a table for this repo."
    />
  {/if}

  {#if adding}
    <SettingsRow always layout="col">
      <RiskRuleForm
        chooseSource
        projectAllowed={!!repo}
        submitLabel="Add rule"
        {busy}
        onsubmit={(draft) => save(draft)}
        oncancel={() => (adding = false)}
      />
    </SettingsRow>
  {/if}

  <SettingsRow
    always
    description="Most specific wins: exact path, then glob, then file type. This repo's rules come before yours, and anything unclaimed reads as {tierLabel(defaultTier)}."
  >
    {#if !adding}
      <SettingsButton
        onclick={() => {
          editing = null;
          adding = true;
        }}
      >+ Add rule</SettingsButton>
    {/if}
    <SettingsButton
      variant="ai"
      title="An agent reads the repo, prints how many files each tier captures, and replaces this repo's rules. It takes a few minutes."
      onclick={onpropose}
    >✦ Propose rules</SettingsButton>
  </SettingsRow>

  {#if files.length > 0}
    <SettingsRow always layout="col">
      <button
        type="button"
        class="self-start text-xs text-fg-3 hover:text-fg"
        aria-expanded={showFiles}
        onclick={() => (showFiles = !showFiles)}
      >{showFiles ? "▾" : "▸"} Changed files in this tab · {files.length}</button>
      {#if showFiles}
        <ul class="grid gap-0.5" aria-label="Changed files and the rule that claimed each">
          {#each fileRows.shown as file (file.path)}
            <li class="grid grid-cols-[minmax(0,1fr)_auto_52px] gap-2 items-baseline text-[11px]">
              <span class="font-mono text-fg-2 truncate" title={file.path}>{file.path}</span>
              <span
                class="font-mono text-muted truncate max-w-[200px]"
                title={file.rule ? `matched by ${file.rule.key}` : `no rule matched — reads as ${tierLabel(file.tier)}`}
              >{claimedBy(file)}</span>
              <RiskTierPill tier={file.tier} />
            </li>
          {/each}
        </ul>
        {#if fileRows.hidden > 0}
          <p class="text-[11px] text-muted">{fileRows.hidden} more changed files not listed.</p>
        {/if}
      {/if}
    </SettingsRow>
  {/if}
</SettingsSection>
