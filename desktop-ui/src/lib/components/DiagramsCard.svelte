<script lang="ts">
  import type { AiSnapshot, DiagramSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import Card from "./ui/Card.svelte";
  import ModalShell from "./ui/ModalShell.svelte";
  import MermaidDiagram from "./MermaidDiagram.svelte";

  interface Props {
    ai: AiSnapshot;
  }

  const { ai }: Props = $props();

  const presets = $derived(ai.diagram_presets);
  const diagrams = $derived(ai.diagrams);

  // A diagram task's kind is `diagram:<kind>`; correlate running/queued tasks
  // with the preset buttons so each shows its own spinner.
  const diagramTasks = $derived(
    (app.snapshot?.background_tasks ?? []).filter(
      (t) => t.kind.startsWith("diagram:") && (t.status === "running" || t.status === "queued"),
    ),
  );

  function taskFor(kind: string) {
    return diagramTasks.find((t) => t.kind === `diagram:${kind}`);
  }

  let customPrompt = $state("");
  let customOpen = $state(false);
  let expandedId = $state<string | null>(null);
  let confirmDeleteId = $state<string | null>(null);

  const expandedDiagram = $derived(diagrams.find((d) => d.id === expandedId) ?? null);

  function generate(kind: string, prompt?: string) {
    void app.cmd("generate_diagram", { kind, custom_prompt: prompt ?? null });
  }

  function generateCustom() {
    const p = customPrompt.trim();
    if (!p) return;
    generate("custom", p);
    customPrompt = "";
    customOpen = false;
  }

  function regenerate(d: DiagramSnapshot) {
    generate(d.kind, d.kind === "custom" ? d.prompt : undefined);
  }

  async function copyMermaid(d: DiagramSnapshot) {
    await copyToClipboard(d.mermaid);
    app.showToast("success", `Copied mermaid source for "${d.title || d.kind}"`);
  }

  function removeDiagram(d: DiagramSnapshot) {
    if (confirmDeleteId !== d.id) {
      confirmDeleteId = d.id;
      return;
    }
    confirmDeleteId = null;
    void app.cmd("delete_diagram", { id: d.id });
  }

  function kindLabel(kind: string): string {
    return presets.find((p) => p.kind === kind)?.label ?? "Custom";
  }

  function formatCreated(iso: string): string {
    if (!iso) return "";
    const date = new Date(iso);
    if (Number.isNaN(date.getTime())) return "";
    return date.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
</script>

<!-- ── Generate ──────────────────────────────────────────────────────────── -->
<Card>
  <p class="text-[11px] font-semibold text-fg mb-1">Diagrams</p>
  <p class="text-[10px] text-muted leading-relaxed mb-3">
    AI-generated mermaid diagrams of the current diff. Saved per branch/PR in
    your local review storage — never pushed.
  </p>
  <div class="space-y-1.5">
    {#each presets as preset (preset.kind)}
      {@const task = taskFor(preset.kind)}
      <button
        type="button"
        onclick={() => generate(preset.kind)}
        disabled={!!task}
        class="w-full flex items-center gap-2.5 px-2.5 py-2 rounded-lg border border-hairline text-left transition-colors hover:bg-hover hover:border-border disabled:opacity-60 disabled:cursor-default"
      >
        {#if task}
          <svg class="animate-spin shrink-0 text-accent" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
          </svg>
        {:else}
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 text-accent">
            <rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/><path d="M10 6.5h5.5a2 2 0 0 1 2 2V14"/>
          </svg>
        {/if}
        <span class="flex-1 min-w-0">
          <span class="block text-[11px] font-medium text-fg">
            {preset.label}
            {#if task}
              <span class="ml-1 text-[9px] font-normal text-fg-3">
                {task.status === "queued" ? "queued" : "generating…"}
              </span>
            {/if}
          </span>
          <span class="block text-[10px] text-fg-3 leading-snug">{preset.description}</span>
        </span>
      </button>
    {/each}

    <!-- Custom prompt -->
    {#if customOpen}
      <div class="rounded-lg border border-border bg-surface p-2.5 space-y-2">
        <textarea
          bind:value={customPrompt}
          rows="3"
          placeholder="e.g. Sequence diagram of the auth token refresh path"
          class="w-full bg-transparent text-[11px] text-fg placeholder:text-muted resize-y outline-none"
          onkeydown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              generateCustom();
            }
          }}
        ></textarea>
        <div class="flex items-center justify-end gap-1.5">
          <button
            type="button"
            onclick={() => { customOpen = false; }}
            class="px-2 py-1 text-[10px] rounded border border-hairline text-fg-3 hover:text-fg-2 hover:bg-hover transition-colors"
          >Cancel</button>
          <button
            type="button"
            onclick={generateCustom}
            disabled={!customPrompt.trim() || !!taskFor("custom")}
            class="px-2 py-1 text-[10px] font-medium rounded bg-accent text-on-accent hover:bg-accent/90 transition-colors disabled:opacity-50 disabled:cursor-default"
          >Generate (⌘⏎)</button>
        </div>
      </div>
    {:else}
      <button
        type="button"
        onclick={() => { customOpen = true; }}
        class="w-full flex items-center gap-2.5 px-2.5 py-2 rounded-lg border border-dashed border-hairline text-left transition-colors hover:bg-hover hover:border-border"
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 text-fg-3">
          <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
        </svg>
        <span class="flex-1 min-w-0">
          <span class="block text-[11px] font-medium text-fg-2">Custom diagram…</span>
          <span class="block text-[10px] text-fg-3 leading-snug">Describe the diagram you want</span>
        </span>
      </button>
    {/if}
  </div>
</Card>

<!-- ── Generated diagrams ────────────────────────────────────────────────── -->
{#if diagrams.length > 0}
  <div class="space-y-3">
    {#each diagrams as d (d.id)}
      {@const task = taskFor(d.kind)}
      <Card class="p-3">
        <div class="flex items-start gap-2 mb-2">
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-1.5">
              <span class="text-[9px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded bg-accent/15 text-accent">
                {kindLabel(d.kind)}
              </span>
              {#if !d.fresh}
                <span
                  class="text-[9px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded bg-risk-med/15 text-risk-med"
                  title="Generated for an older diff — regenerate to refresh"
                >stale</span>
              {/if}
            </div>
            <p class="text-[11px] font-medium text-fg mt-1 leading-snug break-words">
              {d.title || kindLabel(d.kind)}
            </p>
            {#if d.created_at}
              <p class="text-[9px] text-muted mt-0.5">{formatCreated(d.created_at)}</p>
            {/if}
            {#if d.kind === "custom" && d.prompt}
              <p class="text-[10px] text-fg-3 mt-1 italic leading-snug break-words">"{d.prompt}"</p>
            {/if}
          </div>
          <!-- Row actions -->
          <div class="flex items-center gap-0.5 shrink-0">
            <button
              type="button"
              onclick={() => regenerate(d)}
              disabled={!!task}
              title="Regenerate this diagram"
              class="p-1.5 rounded text-fg-3 hover:bg-hover hover:text-fg-2 transition-colors disabled:opacity-50"
            >
              {#if task}
                <svg class="animate-spin" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
                </svg>
              {:else}
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/>
                </svg>
              {/if}
            </button>
            <button
              type="button"
              onclick={() => void copyMermaid(d)}
              title="Copy mermaid source"
              class="p-1.5 rounded text-fg-3 hover:bg-hover hover:text-fg-2 transition-colors"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
              </svg>
            </button>
            <button
              type="button"
              onclick={() => removeDiagram(d)}
              title={confirmDeleteId === d.id ? "Click again to confirm delete" : "Delete diagram"}
              class="p-1.5 rounded transition-colors {confirmDeleteId === d.id ? 'text-del-fg bg-del-bg' : 'text-fg-3 hover:bg-hover hover:text-del-fg'}"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
              </svg>
            </button>
          </div>
        </div>

        <!-- Click to expand -->
        <button
          type="button"
          onclick={() => { expandedId = d.id; }}
          title="Expand diagram"
          class="block w-full rounded-lg border border-hairline bg-surface p-2 cursor-zoom-in hover:border-border transition-colors"
        >
          <MermaidDiagram source={d.mermaid} class="max-h-64 pointer-events-none" />
        </button>
      </Card>
    {/each}
  </div>
{/if}

<!-- ── Expanded diagram modal ────────────────────────────────────────────── -->
<ModalShell
  open={expandedDiagram !== null}
  ariaLabel={expandedDiagram?.title ?? "Diagram"}
  onClose={() => { expandedId = null; }}
  backdropClass="fixed inset-0 z-50 bg-bg/60 p-6"
  panelClass="fixed left-1/2 top-1/2 z-[51] w-[min(92vw,1100px)] max-h-[85vh] -translate-x-1/2 -translate-y-1/2 rounded-xl border border-border bg-surface shadow-2xl outline-none flex flex-col"
>
  {#if expandedDiagram}
    <div class="flex items-center gap-2 px-4 py-3 border-b border-hairline shrink-0">
      <span class="text-[9px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded bg-accent/15 text-accent">
        {kindLabel(expandedDiagram.kind)}
      </span>
      <p class="flex-1 min-w-0 text-[12px] font-medium text-fg truncate">
        {expandedDiagram.title || kindLabel(expandedDiagram.kind)}
      </p>
      {#if !expandedDiagram.fresh}
        <span class="text-[9px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded bg-risk-med/15 text-risk-med">stale</span>
      {/if}
      <button
        type="button"
        onclick={() => { if (expandedDiagram) void copyMermaid(expandedDiagram); }}
        class="px-2 py-1 text-[10px] rounded border border-hairline text-fg-3 hover:text-fg-2 hover:bg-hover transition-colors"
      >Copy source</button>
      <button
        type="button"
        onclick={() => { expandedId = null; }}
        aria-label="Close"
        class="p-1 rounded text-fg-3 hover:text-fg hover:bg-hover transition-colors"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
        </svg>
      </button>
    </div>
    <div class="flex-1 overflow-auto p-6">
      <MermaidDiagram source={expandedDiagram.mermaid} />
    </div>
  {/if}
</ModalShell>
