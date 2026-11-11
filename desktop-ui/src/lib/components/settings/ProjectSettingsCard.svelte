<script lang="ts">
  import type { ProjectSnapshot } from "$lib/types";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsRow from "./SettingsRow.svelte";
  import SettingsButton from "./SettingsButton.svelte";
  import { INPUT_CLASS } from "./settingsModel";
  import { PROJECT_ROWS } from "./settingsSearch";

  interface Props {
    project: ProjectSnapshot;
    onpatch: (patch: Record<string, unknown>) => Promise<void>;
  }

  const { project, onpatch }: Props = $props();

  let addGlob = $state("");

  const maxDiffKb = $derived(project.auto_triage_max_diff_kb ?? 0);
  const ignoreGlobs = $derived(project.review_ignore_globs ?? []);

  // Editable draft; resets whenever the saved value changes.
  let maxDiffInput = $derived(maxDiffKb === 0 ? "" : String(maxDiffKb));

  function commitMaxDiff() {
    const trimmed = maxDiffInput.trim();
    const kb = trimmed === "" ? 0 : Math.max(0, parseInt(trimmed, 10) || 0);
    if (kb !== maxDiffKb) void onpatch({ autoTriageMaxDiffKb: kb });
  }

  async function addIgnore() {
    const glob = addGlob.trim();
    if (!glob) return;
    await onpatch({ reviewIgnoreGlobAdd: glob });
    addGlob = "";
  }
</script>

<SettingsSection title={project.name}>
  <SettingsRow {...PROJECT_ROWS.location}>
    <span class="font-mono text-xs text-fg-3 truncate max-w-[360px]" title={project.root_path || project.remote || ""}>
      {project.root_path || project.remote || "—"}
    </span>
  </SettingsRow>

  {#if project.remote}
    <SettingsRow {...PROJECT_ROWS.sizeLimit}>
      <span class="inline-flex items-center gap-1.5">
        <input
          type="number"
          min="0"
          step="1"
          class="{INPUT_CLASS} w-[96px] flex-none text-right"
          aria-label="Size limit in KB"
          placeholder="No limit"
          bind:value={maxDiffInput}
          onchange={commitMaxDiff}
          onkeydown={(e) => e.stopPropagation()}
        />
        <span class="text-xs text-muted">KB</span>
      </span>
    </SettingsRow>
  {/if}

  <SettingsRow {...PROJECT_ROWS.ignore} keywords={ignoreGlobs} layout="col">
    {#if ignoreGlobs.length > 0}
      <ul class="w-full grid gap-px" aria-label="Ignore patterns">
        {#each ignoreGlobs as glob, index (glob + index)}
          <li class="flex items-center gap-2 font-mono text-xs text-fg-2">
            <span class="flex-1 truncate" title={glob}>{glob}</span>
            <button
              type="button"
              class="w-[22px] h-[22px] grid place-items-center rounded-[5px] text-[15px] leading-none text-fg-3 hover:bg-hover hover:text-risk-high"
              aria-label="Remove {glob}"
              onclick={() => void onpatch({ reviewIgnoreGlobRemove: index })}
            >×</button>
          </li>
        {/each}
      </ul>
    {/if}
    <form
      class="flex w-full gap-2"
      onsubmit={(e) => {
        e.preventDefault();
        void addIgnore();
      }}
    >
      <input
        type="text"
        class={INPUT_CLASS}
        aria-label="Add ignore pattern"
        placeholder="Glob pattern, e.g. package-lock.json"
        bind:value={addGlob}
        onkeydown={(e) => e.stopPropagation()}
      />
      <SettingsButton type="submit" disabled={!addGlob.trim()}>Add</SettingsButton>
    </form>
  </SettingsRow>
</SettingsSection>
