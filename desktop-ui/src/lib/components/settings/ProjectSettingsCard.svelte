<script lang="ts">
  import type { ProjectSnapshot } from "$lib/types";
  import Button from "$lib/components/ui/Button.svelte";

  interface Props {
    project: ProjectSnapshot;
    onpatch: (patch: Record<string, unknown>) => Promise<void>;
  }

  const { project, onpatch }: Props = $props();

  let addGlob = $state("");
  let maxDiffInput = $state("");

  const maxDiffKb = $derived(project.auto_triage_max_diff_kb ?? 0);
  const ignoreGlobs = $derived(project.review_ignore_globs ?? []);

  $effect(() => {
    maxDiffInput = maxDiffKb === 0 ? "" : String(maxDiffKb);
  });

</script>

<section class="bg-card border border-hairline rounded-xl px-4 py-3.5 mb-4">
  <div class="flex items-baseline justify-between gap-2">
    <h3 class="text-sm font-semibold text-fg tracking-tight">{project.name}</h3>
    {#if project.remote}
      <span class="text-[10px] font-mono text-muted truncate">{project.remote}</span>
    {/if}
  </div>
  {#if project.root_path}
    <p class="text-[10px] font-mono text-muted truncate mt-0.5 mb-2" title={project.root_path}>
      {project.root_path}
    </p>
  {/if}

  {#if project.remote}
    <p class="text-xs text-muted mb-2">
      Run triage from the sidebar — hover a branch or PR row and click the scan icon.
    </p>
    <div class="py-2">
      <div class="text-sm text-fg mb-0.5">Max diff size (KB)</div>
      <div class="text-xs text-muted mb-1.5">
        Skip sidebar triage when the filtered diff exceeds this size. Leave empty for no limit.
      </div>
      <div class="flex gap-2 max-w-xs">
        <input
          type="number"
          min="0"
          step="1"
          class="flex-1 bg-ink-850 border border-hairline rounded-md px-2.5 py-1.5 text-sm font-mono outline-none transition-colors placeholder:text-ink-300 hover:border-border focus:border-accent/60"
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
  {/if}

  <div class="mt-3 pt-3 border-t border-hairline/60">
    <div class="text-sm text-fg mb-0.5">Ignore in AI reviews</div>
    <div class="text-xs text-muted mb-2">
      Glob patterns excluded from triage and full-review diffs (e.g. <code class="font-mono">**/*.lock</code>).
    </div>
    {#if ignoreGlobs.length === 0}
      <p class="text-xs text-muted/80 italic mb-2">No ignore patterns.</p>
    {:else}
      <div class="divide-y divide-hairline/40 mb-1">
        {#each ignoreGlobs as glob, index (glob + index)}
          <div class="flex items-center gap-2 py-1.5 font-mono text-xs text-fg-2 group">
            <span class="flex-1 truncate" title={glob}>{glob}</span>
            <button
              type="button"
              class="text-muted opacity-60 group-hover:opacity-100 hover:text-risk-high px-2 py-0.5 rounded transition-colors"
              title="Remove"
              onclick={() => onpatch({ reviewIgnoreGlobRemove: index })}
            >
              ×
            </button>
          </div>
        {/each}
      </div>
    {/if}
    <div class="flex gap-2 mt-1">
      <input
        type="text"
        class="flex-1 bg-ink-850 border border-hairline rounded-md px-2.5 py-1.5 text-sm font-mono outline-none transition-colors placeholder:text-ink-300 hover:border-border focus:border-accent/60"
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
