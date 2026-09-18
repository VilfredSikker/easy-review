<script lang="ts">
  import type { ChecklistSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { checklistProgress, groupChecklistItems } from "$lib/checklist";
  import { navigateToFinding } from "$lib/dom";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import { tick } from "svelte";

  interface Props {
    checklist: ChecklistSnapshot | null;
  }

  const { checklist }: Props = $props();

  const items = $derived(checklist?.items ?? []);
  const groups = $derived(groupChecklistItems(items));
  const progress = $derived(checklistProgress(items));
  const reviewScope = $derived(reviewScopeFromMode(app.snapshot?.mode));

  // The address is the item's position in the flat list — grouping moves rows
  // around on screen without changing what a click targets.
  function toggle(index: number) {
    void app.cmd("toggle_checklist_item", { index });
  }

  async function jumpToFile(path: string) {
    const snap = app.snapshot;
    if (!snap) return;
    const f = snap.files.find((file) => file.path === path);
    if (f) {
      await app.cmd("select_file", { idx: f.source_index });
      await tick();
    }
  }

  // A related finding can be gone (dismissed, dropped by the arbiter) while the
  // item that referred to it stays; the id is then all there is to show.
  function findingFor(id: string) {
    return app.snapshot?.ai.findings.find((f) => f.id === id) ?? null;
  }

  function jumpToFinding(id: string) {
    const finding = findingFor(id);
    if (finding) navigateToFinding(finding);
  }

  function basename(path: string): string {
    const i = path.lastIndexOf("/");
    return i === -1 ? path : path.slice(i + 1);
  }

  function runReview() {
    if (!reviewScope) return;
    void app.cmd("run_ai_review", { scope: reviewScope });
  }
</script>

<Card>
  <div class="flex items-center justify-between gap-2">
    <SectionLabel>Checklist</SectionLabel>
    <div class="flex items-center gap-1.5">
      {#if checklist && !checklist.fresh}
        <span
          class="text-[9px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded bg-risk-med/15 text-risk-med"
          title="Generated for an older diff — re-run the review to refresh it"
        >stale</span>
      {/if}
      {#if items.length > 0}
        <span class="text-[10px] tabular-nums text-muted" aria-label="{progress} items checked">
          {progress}
        </span>
      {/if}
    </div>
  </div>

  {#if items.length === 0}
    <p class="mt-2 text-[11px] text-muted leading-relaxed">
      No checklist for this view. A general review writes one — the outcomes worth
      confirming by hand, rather than a second reading of the diff.
    </p>
    {#if reviewScope}
      <Button class="mt-2" onclick={runReview}>Run review</Button>
    {/if}
  {:else}
    <div class="mt-2.5 max-h-72 space-y-2.5 overflow-y-auto">
      {#each groups as group (group.category)}
        <div class="min-w-0">
          <p class="mb-1 text-[10px] uppercase tracking-wide text-muted">
            {group.label}
          </p>
          <ul class="space-y-0.5">
            {#each group.rows as row (row.item.id)}
              <li class="min-w-0 rounded-md px-1 py-1 hover:bg-bg">
                <div class="flex items-start gap-1.5">
                  <button
                    type="button"
                    role="checkbox"
                    aria-checked={row.item.checked}
                    aria-label={row.item.text}
                    title={row.item.checked ? "Mark as not checked" : "Mark as checked"}
                    onclick={() => toggle(row.index)}
                    class="mt-[1px] grid h-3.5 w-3.5 shrink-0 place-items-center rounded border transition-colors
                      {row.item.checked
                        ? 'border-accent bg-accent/20 text-accent'
                        : 'border-border text-transparent hover:border-accent/60'}"
                  >
                    <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="20 6 9 17 4 12"/>
                    </svg>
                  </button>
                  <span
                    class="min-w-0 flex-1 text-[11px] leading-snug {row.item.checked
                      ? 'text-muted'
                      : 'text-fg-2'}"
                  >{row.item.text}</span>
                </div>

                {#if row.item.related_files.length > 0 || row.item.related_findings.length > 0}
                  <div class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 pl-5">
                    {#each row.item.related_files as path (path)}
                      <button
                        type="button"
                        class="max-w-[180px] truncate font-mono text-[10px] text-fg-3 hover:text-accent transition-colors"
                        title={path}
                        onclick={() => jumpToFile(path)}
                      >{basename(path)}</button>
                    {/each}
                    {#each row.item.related_findings as findingId (findingId)}
                      {@const finding = findingFor(findingId)}
                      <button
                        type="button"
                        class="max-w-[180px] truncate text-[10px] text-finding hover:text-accent transition-colors"
                        title={finding?.title ?? findingId}
                        onclick={() => jumpToFinding(findingId)}
                      >{finding?.title ?? findingId}</button>
                    {/each}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    </div>
  {/if}
</Card>
