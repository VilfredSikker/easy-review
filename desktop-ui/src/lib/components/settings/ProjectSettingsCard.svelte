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

  {#if project.remote}
    <p class="text-xs text-muted mb-2">
      Run triage from the sidebar — hover a branch or PR row and click the scan icon.
    </p>
    <div class="py-2">
      <div class="text-sm text-fg mb-1">Max diff size (KB)</div>
      <div class="text-xs text-muted mb-1.5">
        Skip sidebar triage when the filtered diff exceeds this size. Leave empty for no limit.
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
