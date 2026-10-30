<script lang="ts">
  import type { ProjectSnapshot } from "$lib/types";
  import Segmented from "./Segmented.svelte";
  import ProjectSettingsCard from "./ProjectSettingsCard.svelte";

  interface Props {
    projects: ProjectSnapshot[];
    onpatch: (projectId: string, patch: Record<string, unknown>) => Promise<void>;
  }

  const { projects, onpatch }: Props = $props();

  let picked = $state<string | null>(null);
  // One project at a time, opening on the active one; a pick that disappears
  // (project removed) falls back rather than showing nothing.
  const current = $derived(
    projects.find((p) => p.id === picked) ?? projects.find((p) => p.is_active) ?? projects[0] ?? null,
  );
</script>

{#if !current}
  <div class="border border-dashed border-border rounded-[10px] px-6 py-10 text-center">
    <p class="text-[13px] text-fg-3 mb-1">No projects registered yet</p>
    <p class="text-xs text-muted">Open a repo from the sidebar first.</p>
  </div>
{:else}
  {#if projects.length > 1}
    <div class="mb-3">
      <Segmented
        options={projects.map((p) => p.id)}
        value={current.id}
        ariaLabel="Project"
        display={(id) => projects.find((p) => p.id === id)?.name ?? id}
        onchange={(id) => (picked = id)}
      />
    </div>
  {/if}
  {#key current.id}
    <ProjectSettingsCard project={current} onpatch={(changes) => onpatch(current.id, changes)} />
  {/key}
{/if}
