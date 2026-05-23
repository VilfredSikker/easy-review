<script lang="ts">
  import { FileTree } from "@pierre/trees";
  import type { FileSnapshot } from "$lib/types";
  import {
    createDecorationState,
    erTreesHostStyles,
    filePaths,
    prepareTreesInput,
    readOnlyTreesOptions,
    toGitStatusEntries,
  } from "$lib/spikes/treesFileTree";
  import { onMount } from "svelte";

  interface Props {
    files: FileSnapshot[];
    /** Highlight this path on mount / when it changes. */
    selectedPath?: string | null;
    onSelect?: (path: string) => void;
  }

  const { files, selectedPath = null, onSelect }: Props = $props();

  let host: HTMLDivElement | undefined = $state();
  let model: FileTree | null = null;
  const decorationState: ReturnType<typeof createDecorationState> = { byPath: new Map() };
  const hostStyleCss = Object.entries(erTreesHostStyles())
    .map(([k, v]) => `${k}: ${v}`)
    .join("; ");

  function syncModel() {
    if (!model) return;
    decorationState.byPath = new Map(files.map((f) => [f.path, f]));
    const paths = filePaths(files);
    model.setGitStatus(toGitStatusEntries(files));
    model.resetPaths(paths, { preparedInput: prepareTreesInput(files) });
    if (selectedPath) model.focusPath(selectedPath);
  }

  onMount(() => {
    if (!host) return;
    decorationState.byPath = new Map(files.map((f) => [f.path, f]));
    model = new FileTree(readOnlyTreesOptions(files, decorationState, (path) => {
      if (path && onSelect) onSelect(path);
    }));
    model.render({ fileTreeContainer: host });
    if (selectedPath) model.focusPath(selectedPath);
    return () => {
      model?.unmount();
      model = null;
    };
  });

  $effect(() => {
    files;
    selectedPath;
    syncModel();
  });
</script>

<div class="h-full min-h-0 flex flex-col bg-surface border border-hairline">
  <div bind:this={host} class="flex-1 min-h-0" style={hostStyleCss}></div>
</div>
