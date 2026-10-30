<script lang="ts">
  /** Tour ruler: ticks grouped by pillar, so the edge shows how the tour
   *  divides the diff; the popover lists the pillars with their files. */
  import RulerBase from "./RulerBase.svelte";
  import { tourGroups, type RulerFilter } from "$lib/rulerGeometry";
  import type { FileSnapshot, PillarSnapshot } from "$lib/types";

  interface Props {
    pillars: readonly PillarSnapshot[];
    files: readonly FileSnapshot[];
    currentPath: string | null;
    onGoto: (path: string) => void;
    filter?: RulerFilter;
    left?: string;
  }
  const { pillars, files, currentPath, onGoto, filter, left }: Props = $props();

  const groups = $derived(tourGroups(pillars, files, currentPath, filter));
  const pillarCount = $derived(pillars.length);
</script>

<RulerBase
  {groups}
  {onGoto}
  {left}
  label="Tour map"
  heading="Tour · {pillarCount} pillar{pillarCount === 1 ? '' : 's'}"
/>
