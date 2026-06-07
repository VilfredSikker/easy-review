<script lang="ts">
  import type { ProjectSnapshot } from "$lib/types";
  import Toggle from "./Toggle.svelte";
  import OptionGroup from "./OptionGroup.svelte";
  import Button from "$lib/components/ui/Button.svelte";

  const AUTO_TRIAGE_WHEN_OPTIONS = ["new-and-push", "new-only", "review-requested"] as const;

  interface Props {
    project: ProjectSnapshot;
    onpatch: (patch: Record<string, unknown>) => Promise<void>;
  }

  const { project, onpatch }: Props = $props();

  let addGlob = $state("");
  let maxDiffInput = $state("");

  const autoTriageWhen = $derived(project.auto_triage_when ?? "new-and-push");
  const maxDiffKb = $derived(project.auto_triage_max_diff_kb ?? 0);
  const ignoreGlobs = $derived(project.review_ignore_globs ?? []);

  $effect(() => {
    maxDiffInput = maxDiffKb === 0 ? "" : String(maxDiffKb);
  });

  const whenDescriptions: Record<string, string> = {
    "new-and-push": "Triage when a PR is new or its head commit changes.",
    "new-only": "Triage once per PR; skip updates after new commits.",
    "review-requested": "Triage only when you are added as a reviewer.",
  };
</script>

<section class="border border-hairline rounded-lg px-4 py-3 mb-3 bg-surface/40">
  <div class="flex items-baseline justify-between gap-2 mb-1">
    <h3 class="text-sm font-medium text-fg">{project.name}</h3>
    {#if project.remote}
      <span class="text-[10px] font-mono text-muted truncate">{project.remote}</span>
    {/if}
  </div>
  {#if project.root_path}
    <p class="text-[10px] font-mono text-muted truncate mb-2" title={project.root_path}>
      {project.root_path}
    </p>
  {/if}

  {#if !project.remote}
    <p class="text-xs text-muted">No GitHub remote — auto-triage needs a linked remote.</p>
  {:else}
    <Toggle
      label="Auto-triage open PRs"
      description="Run triage while Desktop is open (PR list refresh ~every 10 min)."
      checked={project.auto_triage ?? false}
      onchange={(v) => onpatch({ autoTriage: v })}
    />
    <Toggle
      label="Include my PRs"
      description="Also triage PRs you authored."
      checked={project.auto_triage_own_prs ?? false}
      disabled={!(project.auto_triage ?? false)}
      onchange={(v) => onpatch({ autoTriageOwnPrs: v })}
    />

    <div class={project.auto_triage ? "" : "opacity-50 pointer-events-none"}>
      <OptionGroup
        label="When to triage"
        description={whenDescriptions[autoTriageWhen] ?? whenDescriptions["new-and-push"]}
        options={[...AUTO_TRIAGE_WHEN_OPTIONS]}
        value={autoTriageWhen}
        onchange={(v) => onpatch({ autoTriageWhen: v })}
      />

      <div class="py-2">
        <div class="text-sm text-fg mb-1">Max diff size (KB)</div>
        <div class="text-xs text-muted mb-1.5">
          Skip auto-triage when the filtered diff exceeds this size. Leave empty for no limit.
        </div>
        <div class="flex gap-2 max-w-xs">
          <input
            type="number"
            min="0"
            step="1"
            class="flex-1 bg-surface border border-hairline rounded-md px-2 py-1.5 text-sm font-mono"
            placeholder="No limit"
            bind:value={maxDiffInput}
            onchange={() => {
              const trimmed = maxDiffInput.trim();
              const kb = trimmed === "" ? 0 : Math.max(0, parseInt(trimmed, 10) || 0);
              void onpatch({ autoTriageMaxDiffKb: kb });
            }}
          />
        </div>
      </div>
    </div>
  {/if}

  <div class="mt-3 pt-3 border-t border-hairline/60">
    <div class="text-sm text-fg mb-1">Ignore in AI reviews</div>
    <div class="text-xs text-muted mb-2">
      Glob patterns excluded from triage and full-review diffs (e.g. <code class="font-mono">**/*.lock</code>).
    </div>
    {#if ignoreGlobs.length === 0}
      <p class="text-xs text-muted mb-2">No ignore patterns.</p>
    {:else}
      {#each ignoreGlobs as glob, index (glob + index)}
        <div class="flex items-center gap-2 py-1 font-mono text-xs text-fg-2">
          <span class="flex-1 truncate" title={glob}>{glob}</span>
          <button
            type="button"
            class="text-muted hover:text-red-400 px-2"
            title="Remove"
            onclick={() => onpatch({ reviewIgnoreGlobRemove: index })}
          >
            ×
          </button>
        </div>
      {/each}
    {/if}
    <div class="flex gap-2 mt-1">
      <input
        type="text"
        class="flex-1 bg-surface border border-hairline rounded-md px-2 py-1.5 text-sm font-mono"
        placeholder="Glob pattern, e.g. package-lock.json"
        bind:value={addGlob}
      />
      <Button
        onclick={() => {
          const p = addGlob.trim();
          if (!p) return;
          void onpatch({ reviewIgnoreGlobAdd: p }).then(() => {
            addGlob = "";
          });
        }}
      >
        Add
      </Button>
    </div>
  </div>
</section>
