<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import {
    VERDICT_LABEL,
    findingLabels,
    isAccepted,
    latestSeverity,
    overlayAccess,
    pageStats,
    verdictKey,
  } from "$lib/arena/arenaView";
  import { pendingAcceptanceFindings } from "$lib/arena/projections";
  import { isSeededRunFromSnapshot, seededVerificationNotice } from "$lib/arena/runKind";
  import {
    evidenceExcerpt,
    lensNames,
    severityKey,
    stepSelection,
    verificationGroups,
    verificationMeta,
    verificationNote,
    visibleOrder,
    type VerificationFilter,
  } from "$lib/arena/verificationView";
  import { arena } from "$lib/stores/arena.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { startWindowDrag } from "$lib/windowDrag";
  import ArenaVerdictPill from "$lib/components/arena/ArenaVerdictPill.svelte";
  import VerificationFindingDetail from "$lib/components/arena/VerificationFindingDetail.svelte";

  interface Props {
    snapshot: ArenaRunSnapshot;
    /** Opened from the review flow's own origin, which is what lets Deep results be accepted. */
    flowAcceptance?: boolean;
    onClose: () => void;
  }

  const { snapshot, flowAcceptance = false, onClose }: Props = $props();

  let filter = $state<VerificationFilter>("all");
  let pickedId = $state<string | null>(null);
  let accepting = $state(false);

  const run = $derived(snapshot.run);
  const seeded = $derived(isSeededRunFromSnapshot(snapshot));
  const access = $derived(
    overlayAccess({ flowAcceptance, runKind: run.config.run_kind, readOnlyVerification: seeded }),
  );
  const labels = $derived(findingLabels(run.findings));
  const stats = $derived(pageStats(run.findings));
  const tiles = $derived(
    [
      { key: "all" as const, label: "All", n: stats.proposed, color: "var(--color-fg)" },
      { key: "kept" as const, label: "Kept", n: stats.kept, color: "var(--color-success)" },
      { key: "escalated" as const, label: "Escalated", n: stats.escalated, color: "var(--color-error)" },
      { key: "merged" as const, label: "Merged", n: stats.merged, color: "var(--color-periwinkle)" },
      { key: "dropped" as const, label: "Dropped", n: stats.dropped, color: "var(--color-muted)" },
      { key: "pending" as const, label: "Pending", n: stats.pending, color: "var(--color-warning)" },
    ].filter((t) => t.key !== "pending" || t.n > 0),
  );
  const groups = $derived(verificationGroups(run.findings, filter));
  const order = $derived(visibleOrder(run.findings, filter));
  // A pick hidden by the filter falls back to the first visible finding, so the detail never goes blank.
  const selectedId = $derived(pickedId && order.includes(pickedId) ? pickedId : order[0] ?? null);
  const selected = $derived(run.findings.find((f) => f.id === selectedId) ?? null);

  const excluded = $derived(arena.excludedFor(run.id));
  const canAccept = $derived(flowAcceptance && !access.readOnlyFlow && run.status === "complete");
  const toAccept = $derived(
    canAccept
      ? pendingAcceptanceFindings(run.findings, run.accepted_finding_ids ?? [], true).filter((f) => !excluded.has(f.id))
      : [],
  );
  const note = $derived(
    verificationNote({
      seeded,
      status: run.status,
      readOnlyFlow: access.readOnlyFlow,
      awaiting: toAccept.length,
      seededText: seededVerificationNotice(snapshot),
    }),
  );
  const meta = $derived(verificationMeta(snapshot));
  const excerpt = $derived.by(() => {
    if (!selected) return [];
    const file = app.snapshot?.files.find((f) => f.path === selected.file);
    return file && !file.is_lazy_stub ? evidenceExcerpt(file.hunks, selected) : [];
  });

  function select(id: string) {
    pickedId = id;
    document.getElementById(`vf-row-${id}`)?.scrollIntoView({ block: "nearest" });
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "j" && e.key !== "k") return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      // The launcher opens over this page; its fields own the keyboard.
      if (arena.launcherOpen) return;
      const t = e.target as HTMLElement | null;
      if (t?.closest("input, textarea, select, [contenteditable='true']")) return;
      const next = stepSelection(order, selectedId, e.key === "j" ? 1 : -1);
      if (!next) return;
      e.preventDefault();
      select(next);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function acceptAll() {
    if (toAccept.length === 0) return;
    accepting = true;
    try {
      await arena.acceptFindings(
        run.id,
        toAccept.map((f) => f.id),
      );
    } catch {
      // acceptFindings already told the user what failed.
    } finally {
      accepting = false;
    }
  }

  function setFilter(key: VerificationFilter) {
    filter = filter === key && key !== "all" ? "all" : key;
  }
</script>

<!-- Tauri window-drag region (platform chrome, mouse-only by design); the dialog is aria-labelled -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<header
  class="vf-head titlebar-drag"
  style="padding-left: max(20px, env(titlebar-area-x, 80px))"
  data-tauri-drag-region
  onmousedown={startWindowDrag}
>
  <button type="button" class="btn ghost titlebar-no-drag" onclick={onClose} title="Back (Esc)">‹ Back</button>
  <div class="min-w-0">
    <h2>Verification results</h2>
    <p class="dim">{meta}</p>
  </div>
  <span class="flex-1"></span>
  <div class="acts titlebar-no-drag">
    <button type="button" class="btn" onclick={() => arena.openLauncher()}>+ New run</button>
    {#if toAccept.length > 0}
      <button type="button" class="btn primary" disabled={accepting} onclick={() => void acceptAll()}>
        {accepting ? "Accepting…" : `Accept ${toAccept.length} finding${toAccept.length === 1 ? "" : "s"}`}
      </button>
    {/if}
  </div>
</header>

<div class="vf-stats">
  {#each tiles as t (t.key)}
    <button type="button" class="vf-stat" class:on={filter === t.key} aria-pressed={filter === t.key} onclick={() => setFilter(t.key)}>
      <span>{t.label}</span>
      <b style="color:{t.color}">{t.n}</b>
    </button>
  {/each}
  <div class="vf-note tone-{note.tone}" role="status">{note.text}</div>
</div>

{#if run.findings.length === 0}
  <div class="empty">
    <b>Nothing to verify</b>
    <p class="dim">The finders reported no findings, so the arbiter had nothing to rule on.</p>
  </div>
{:else}
  <div class="vf-split">
    <nav class="vf-list" aria-label="Verified findings">
      {#each groups as g (g.key)}
        <div class="vf-g">{VERDICT_LABEL[g.key]} <span class="faint">{g.findings.length}</span></div>
        {#each g.findings as f (f.id)}
          {@const v = verdictKey(f.verdict)}
          <button
            type="button"
            id="vf-row-{f.id}"
            aria-current={selectedId === f.id ? "true" : undefined}
            class="vf-row v-{v}"
            class:sel={selectedId === f.id}
            onclick={() => select(f.id)}
          >
            <span class="dot lv-{severityKey(latestSeverity(f))}" title="{latestSeverity(f)} severity"></span>
            <span class="vf-main">
              <span class="vf-t">{f.title}</span>
              <span class="vf-s mono"
                >{f.file.split("/").pop()}{#if f.line}:{f.line}{/if} · {lensNames(f, run.reviewers)}{#if isAccepted(run, f)}
                  · in review{/if}</span
              >
            </span>
            <span class="vf-r">
              <ArenaVerdictPill verdict={f.verdict} />
              <span class="vf-conf" title="Confidence {Math.round(f.confidence * 100)}%"
                ><i style="width:{Math.round(f.confidence * 100)}%"></i></span
              >
            </span>
          </button>
        {/each}
      {/each}
    </nav>
    {#if selected}
      <VerificationFindingDetail
        finding={selected}
        {labels}
        reviewers={run.reviewers}
        {excerpt}
        accepted={isAccepted(run, selected)}
        excluded={excluded.has(selected.id)}
        onOpenInDiff={(p) => void arena.openFileInDiff(p)}
        onKeepAnyway={access.keepAnyway ? (id) => void arena.keepAnyway(run.id, id) : undefined}
        onToggleExcluded={canAccept ? (id) => arena.toggleImportExcluded(run.id, id) : undefined}
      />
    {/if}
  </div>
{/if}

<style>
  .vf-head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--color-hairline);
    flex-wrap: wrap;
  }
  .vf-head h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .vf-head p {
    margin: 2px 0 0;
    font-size: 12px;
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .dim {
    color: var(--color-fg-3);
  }
  .faint {
    color: var(--color-muted);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-fg);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--color-fg) 25%, transparent);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn.primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
    border-color: transparent;
  }
  .btn.ghost {
    height: 26px;
    border-color: transparent;
    background: none;
    color: var(--color-fg-3);
  }
  .btn.ghost:hover {
    background: var(--color-card);
    color: var(--color-fg);
  }
  .vf-stats {
    display: flex;
    gap: 8px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--color-hairline);
    flex-wrap: wrap;
    align-items: stretch;
  }
  .vf-stat {
    display: grid;
    gap: 2px;
    padding: 7px 14px;
    border: 1px solid var(--color-hairline);
    border-radius: 9px;
    background: var(--color-surface);
    min-width: 86px;
    text-align: left;
  }
  .vf-stat:hover {
    border-color: var(--color-border);
  }
  .vf-stat span {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-muted);
    font-weight: 600;
  }
  .vf-stat b {
    font-family: var(--font-mono);
    font-size: 18px;
    font-weight: 500;
  }
  .vf-stat.on {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 1px var(--color-accent);
  }
  .vf-note {
    flex: 1;
    min-width: 240px;
    align-self: center;
    font-size: 12.5px;
    padding: 8px 12px;
    border-radius: 8px;
  }
  .tone-ok {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 7%, transparent);
  }
  .tone-warn {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 8%, transparent);
  }
  .tone-err {
    color: var(--color-error);
    background: color-mix(in srgb, var(--color-error) 8%, transparent);
  }
  .tone-info {
    color: var(--color-fg-3);
    background: var(--color-card);
  }
  .vf-split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 400px minmax(0, 1fr);
  }
  .vf-list {
    border-right: 1px solid var(--color-hairline);
    overflow: auto;
    padding: 6px 8px 24px;
    min-height: 0;
  }
  .vf-g {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-muted);
    padding: 12px 8px 4px;
  }
  .vf-row {
    display: grid;
    grid-template-columns: 12px minmax(0, 1fr) auto;
    gap: 10px;
    align-items: center;
    width: 100%;
    padding: 9px 8px;
    border-radius: 8px;
    text-align: left;
    position: relative;
  }
  .vf-row:hover {
    background: var(--color-card);
  }
  .vf-row.sel {
    background: color-mix(in srgb, var(--color-accent) 9%, var(--color-card));
  }
  .vf-row.sel::before {
    content: "";
    position: absolute;
    left: 0;
    top: 10px;
    bottom: 10px;
    width: 2px;
    border-radius: 2px;
    background: var(--color-accent);
  }
  .vf-row.v-dropped .vf-t {
    color: var(--color-muted);
    text-decoration: line-through;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 2px;
  }
  .lv-high {
    background: var(--color-risk-high);
  }
  .lv-med {
    background: var(--color-risk-med);
  }
  .lv-low {
    background: var(--color-risk-low);
  }
  .vf-main {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .vf-t {
    font-size: 13px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .vf-s {
    font-size: 10.5px;
    color: var(--color-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .vf-r {
    display: grid;
    justify-items: end;
    gap: 5px;
  }
  .vf-conf {
    width: 44px;
    height: 3px;
    border-radius: 2px;
    background: var(--color-card);
    overflow: hidden;
  }
  .vf-conf i {
    display: block;
    height: 100%;
    background: var(--color-fg-3);
  }
  .empty {
    margin: 40px auto;
    max-width: 440px;
    padding: 24px 16px;
    display: grid;
    gap: 10px;
  }
  .empty p {
    margin: 0;
    font-size: 12.5px;
  }
  @media (max-width: 1000px) {
    .vf-split {
      grid-template-columns: minmax(0, 1fr);
    }
    .vf-list {
      border-right: 0;
      border-bottom: 1px solid var(--color-hairline);
      max-height: 40vh;
    }
  }
</style>
