<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { AiSnapshot, PrSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import { resolveActivePrUrl } from "$lib/prUrl";
  import BranchCard from "./BranchCard.svelte";
  import AiReviewCard from "./AiReviewCard.svelte";
  import TriageCard from "./TriageCard.svelte";
  import CommentsCard from "./CommentsCard.svelte";
  import QuestionsCard from "./QuestionsCard.svelte";
  import UiAnnotationsCard from "./UiAnnotationsCard.svelte";
  import AgentOutputCard from "./AgentOutputCard.svelte";

  interface Props {
    ai: AiSnapshot | null;
    pr: PrSnapshot | null;
    width?: number;
    dragging?: boolean;
    onResizeStart?: (e: MouseEvent) => void;
    onCollapseToggle?: () => void;
  }

  const {
    ai,
    pr,
    width = 340,
    dragging = false,
    onResizeStart,
    onCollapseToggle,
  }: Props = $props();

  // ── Tab state (persisted to localStorage) ──────────────────────────────────
  const TAB_STORAGE_KEY = "rightPanelActiveTab";

  type Tab = "branch" | "review" | "notes";

  function readStoredTab(): Tab {
    try {
      const raw = localStorage.getItem(TAB_STORAGE_KEY);
      if (raw === "branch" || raw === "review" || raw === "notes") return raw;
    } catch { /* ignore */ }
    return "branch";
  }

  let activeTab = $state<Tab>(readStoredTab());

  function setTab(t: Tab) {
    activeTab = t;
    try {
      localStorage.setItem(TAB_STORAGE_KEY, t);
    } catch { /* ignore */ }
  }

  // ── Derived counts for tab badges ──────────────────────────────────────────
  const totalFindings = $derived(ai?.findings.length ?? 0);
  const questionCount = $derived(
    ai?.threads.filter((t) => t.kind === "question").length ?? 0
  );

  const currentWorktree = $derived(
    app.snapshot?.worktrees.find((w) => w.is_current) ?? null
  );

  const activeAppTab = $derived(app.snapshot?.tabs?.find((t) => t.is_active) ?? null);
  const displayPrNumber = $derived(
    currentWorktree?.pr_number ?? app.snapshot?.github?.number ?? pr?.number ?? activeAppTab?.pr_number ?? null,
  );
  const displayPrUrl = $derived(resolveActivePrUrl(app.snapshot));

  const checksStatus = $derived.by((): "success" | "pending" | "failure" | null => {
    const checks = app.snapshot?.github?.checks;
    if (!checks || checks.length === 0) return null;
    if (checks.some((c) => c.conclusion === "FAILURE" || c.conclusion === "fail")) return "failure";
    if (checks.some((c) => c.status === "PENDING")) return "pending";
    return "success";
  });

  const totalAdds = $derived(
    app.snapshot?.files.reduce((sum, f) => sum + f.additions, 0) ?? 0
  );
  const totalDels = $derived(
    app.snapshot?.files.reduce((sum, f) => sum + f.deletions, 0) ?? 0
  );

  // ── Tab definitions ─────────────────────────────────────────────────────────
  interface TabDef {
    id: Tab;
    label: string;
    badge: number | null;
  }

  const commentCount = $derived(
    ai?.threads.filter((t) => t.kind === "comment").length ?? 0
  );

  const tabs: TabDef[] = $derived([
    { id: "branch", label: "Branch", badge: commentCount > 0 ? commentCount : null },
    { id: "review", label: "Review", badge: totalFindings > 0 ? totalFindings : null },
    { id: "notes",  label: "Notes",  badge: questionCount > 0 ? questionCount : null },
  ]);

  // ── Per-tab export ───────────────────────────────────────────────────────────
  // The shared `export_review` backend command renders selected sections to
  // markdown. Each tab exports only the sections it shows, so the clipboard
  // content lands ready to paste into a coding agent.
  //
  // NOTE: This mirrors the Rust `ExportOpts` struct in
  // `crates/er-desktop/src/export.rs` (camelCase over IPC). There are no
  // generated Tauri bindings, so keep these fields in sync with that struct —
  // `ExportReviewView.svelte` keeps its own copy for the same reason.
  type ExportOpts = {
    includeComments: boolean;
    includeQuestions: boolean;
    includeFindings: boolean;
    includeAnnotations: boolean;
    onlyUnresolved: boolean;
  };

  const NO_SECTIONS: ExportOpts = {
    includeComments: false,
    includeQuestions: false,
    includeFindings: false,
    includeAnnotations: false,
    onlyUnresolved: false,
  };

  function exportOptsForTab(t: Tab): ExportOpts {
    switch (t) {
      case "branch":
        return { ...NO_SECTIONS, includeComments: true };
      case "review":
        return { ...NO_SECTIONS, includeFindings: true };
      case "notes":
        return { ...NO_SECTIONS, includeQuestions: true, includeAnnotations: true };
    }
  }

  let copying = $state(false);

  async function copyTabToClipboard() {
    if (copying) return;
    copying = true;
    const label = tabs.find((t) => t.id === activeTab)?.label ?? "section";
    try {
      const body = await invoke<string>("export_review", { opts: exportOptsForTab(activeTab) });
      if (!body.trim()) {
        app.showToast("info", `Nothing to export from the ${label} tab`);
        return;
      }
      await copyToClipboard(body);
      app.showToast("success", `Copied ${label} to clipboard (${body.length} chars)`);
    } catch (e) {
      app.showToast("error", `Export failed: ${e}`);
    } finally {
      copying = false;
    }
  }
</script>

<aside
  class="shrink-0 bg-surface border-l border-hairline overflow-hidden flex flex-col relative"
  style="width: {width}px"
>
  <!--
    4px drag handle along the panel's left edge.
  -->
  {#if onResizeStart}
    <div
      class="absolute -left-[2px] top-0 bottom-0 w-1 cursor-ew-resize z-10 hover:bg-accent/40 {dragging ? 'bg-accent/60' : ''}"
      onmousedown={onResizeStart}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize right panel"
    ></div>
  {/if}

  <!-- ── Tab header row ──────────────────────────────────────────────────── -->
  <div class="flex items-stretch border-b border-hairline bg-surface shrink-0">
    {#each tabs as tab}
      {@const isActive = tab.id === activeTab}
      <button
        type="button"
        onclick={() => setTab(tab.id)}
        class="relative flex-1 flex items-center justify-center gap-1.5 px-1.5 py-2.5 text-[11px] font-medium transition-colors
          {isActive ? 'text-fg' : 'text-fg-3 hover:text-fg-2'}"
      >
        <!-- icon -->
        {#if tab.id === "branch"}
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            class={isActive ? "text-accent" : "text-fg-3"}>
            <line x1="6" y1="3" x2="6" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/>
            <path d="M18 9a9 9 0 0 1-9 9"/>
          </svg>
        {:else if tab.id === "review"}
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
            class={isActive ? "text-accent" : "text-fg-3"}>
            <path d="M12 2l2.4 7.2H22l-6.2 4.5 2.4 7.2L12 17l-6.2 3.9 2.4-7.2L2 9.2h7.6z"/>
          </svg>
        {:else}
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            class={isActive ? "text-accent" : "text-fg-3"}>
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
          </svg>
        {/if}

        <span>{tab.label}</span>

        {#if tab.badge !== null}
          <span class="min-w-[14px] h-[14px] px-1 flex items-center justify-center rounded-full text-[9px] font-bold leading-none
            {isActive ? 'bg-accent/20 text-accent' : 'bg-hairline text-fg-3'}">
            {tab.badge}
          </span>
        {/if}

        <!-- active underline -->
        {#if isActive}
          <span class="absolute inset-x-3 bottom-0 h-[2px] bg-accent rounded-t"></span>
        {/if}
      </button>
    {/each}

    <!-- Collapse toggle — always shows "collapse" chevron (panel is expanded when visible) -->
    {#if onCollapseToggle}
      <button
        type="button"
        onclick={onCollapseToggle}
        title="Collapse right panel"
        aria-label="Collapse right panel"
        class="w-8 flex items-center justify-center text-muted hover:text-fg-2 transition-colors shrink-0 border-l border-hairline"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="9 18 15 12 9 6"/>
        </svg>
      </button>
    {/if}
  </div>

  <!-- ── Tab content ─────────────────────────────────────────────────────── -->
  <!-- pb-14 clears the absolutely-positioned export action bar below. -->
  <div class="flex-1 overflow-y-auto pb-14">
    <!-- Branch tab -->
    {#if activeTab === "branch"}
      <div class="p-4 space-y-4 pb-8">
        {#if app.snapshot}
          <BranchCard
            branch={app.snapshot.branch}
            base={app.snapshot.base}
            {pr}
            reviewed_count={app.snapshot.reviewed_count}
            total_count={app.snapshot.total_count}
            additions={totalAdds}
            deletions={totalDels}
            checks_status={checksStatus}
            is_pr={(currentWorktree?.is_pr ?? false) || displayPrNumber !== null}
            pr_number={displayPrNumber}
            is_merged={currentWorktree?.is_merged ?? false}
            github_url={displayPrUrl}
            github={app.snapshot?.github ?? null}
          />
        {/if}
        {#if ai}
          <CommentsCard {ai} active={activeTab === "branch"} />
        {/if}
      </div>

    <!-- Review tab -->
    {:else if activeTab === "review"}
      <div class="p-4 space-y-4 pb-8">
        {#if ai}
          {#if ai.triage}
            <TriageCard triage={ai.triage} />
          {/if}
          <AiReviewCard {ai} />
        {/if}
        <AgentOutputCard />
      </div>

    <!-- Notes tab -->
    {:else if activeTab === "notes"}
      <div class="p-4 pb-8 space-y-4">
        <p class="text-[11px] text-muted leading-relaxed">
          Questions stay on your machine — use them for personal review notes or routing to an AI assistant.
        </p>
        {#if ai && ai.questions > 0}
          <QuestionsCard {ai} />
        {/if}
        <UiAnnotationsCard />
      </div>
    {/if}
  </div>

  <!-- ── Per-tab export actions (pinned to panel bottom) ──────────────────── -->
  <div class="absolute inset-x-0 bottom-0 flex items-center gap-2 px-3 py-2 border-t border-hairline bg-surface">
    <button
      type="button"
      onclick={copyTabToClipboard}
      disabled={copying}
      title="Copy this section as markdown, ready to paste into a coding agent"
      class="flex items-center gap-1.5 px-2 py-1 text-[11px] font-medium rounded border border-border text-fg-2 hover:bg-hover hover:text-fg transition-colors disabled:opacity-50 disabled:cursor-default"
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="9" y="9" width="13" height="13" rx="2"/>
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
      </svg>
      <span>{copying ? "Copying…" : "Export to clipboard"}</span>
    </button>
    <button
      type="button"
      onclick={() => app.setMainView("export-review")}
      title="Open the full export panel"
      class="flex items-center gap-1.5 px-2 py-1 text-[11px] font-medium rounded border border-border text-fg-2 hover:bg-hover hover:text-fg transition-colors"
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>
        <polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/>
      </svg>
      <span>Open export panel</span>
    </button>
  </div>
</aside>
