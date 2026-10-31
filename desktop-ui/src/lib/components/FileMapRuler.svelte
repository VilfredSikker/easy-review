<script lang="ts">
  /** Diff ruler: one flat tick per file in the list the viewer sees, so the
   *  edge doubles as a file map for a long diff. */
  import RulerBase from "./RulerBase.svelte";
  import { fileMapGroups, type RulerFilter } from "$lib/rulerGeometry";
  import type { FileSnapshot } from "$lib/types";

  interface Props {
    files: readonly FileSnapshot[];
    currentPath: string | null;
    onGoto: (path: string) => void;
    filter?: RulerFilter;
  }
  const { files, currentPath, onGoto, filter }: Props = $props();

  const groups = $derived(fileMapGroups(files, currentPath, filter));
</script>

<RulerBase {groups} {onGoto} label="File map" heading="Files changed" />
