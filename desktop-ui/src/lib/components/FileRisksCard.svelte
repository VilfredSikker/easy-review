<script lang="ts">
  import type { FileRiskSnapshot } from "$lib/types";
  import { tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";

  interface Props {
    risks: FileRiskSnapshot[];
  }

  const { risks }: Props = $props();

  function riskDotClass(risk: FileRiskSnapshot["risk"]): string {
    if (risk === "high") return "bg-risk-high";
    if (risk === "med") return "bg-risk-med";
    return "bg-risk-low";
  }

  async function jumpTo(path: string) {
    const snap = app.snapshot;
    if (!snap) return;
    const f = snap.files.find((file) => file.path === path);
    if (f) {
      await app.cmd("select_file", { idx: f.source_index });
      await tick();
    }
  }
</script>

<Card>
  <SectionLabel>File risks</SectionLabel>
  <ul class="mt-3 max-h-64 space-y-0.5 overflow-y-auto">
    {#each risks as risk (risk.path)}
      <li>
        <button
          type="button"
          class="flex w-full min-w-0 items-center gap-2 rounded-md px-1 py-1 text-left hover:bg-bg"
          title={risk.risk_reason || risk.path}
          aria-label="{risk.risk} risk, {risk.path}"
          onclick={() => jumpTo(risk.path)}
        >
          <span
            class="h-1.5 w-1.5 shrink-0 rounded-full {riskDotClass(risk.risk)}"
            aria-hidden="true"
          ></span>
          <span class="truncate-start min-w-0 flex-1 font-mono text-[11px] text-fg-2">
            <span class="truncate-start-inner">{risk.path}</span>
          </span>
        </button>
      </li>
    {/each}
  </ul>
</Card>
