<script lang="ts">
  import type { PrSnapshot, GithubStatusSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import { openExternalUrl } from "$lib/openExternalUrl";

  interface Props {
    branch: string;
    base: string;
    pr: PrSnapshot | null;
    reviewed_count: number;
    total_count: number;
    additions: number;
    deletions: number;
    checks_status: "success" | "pending" | "failure" | null;
    is_pr?: boolean;
    pr_number?: number | null;
    is_merged?: boolean;
    github_url?: string | null;
    github?: GithubStatusSnapshot | null;
  }

  const {
    branch,
    base,
    pr,
    reviewed_count,
    total_count,
    additions,
    deletions,
    checks_status,
    is_pr = false,
    pr_number = null,
    is_merged = false,
    github_url = null,
    github = null,
  }: Props = $props();

  let expandChecks = $state(false);
  let descriptionOpen = $state(false);
  let actionsOpen = $state(false);
  let reviewBody = $state("");
  let submitting = $state(false);

  // First non-empty paragraph of the body, stripped of markdown syntax.
  function descriptionPreview(body: string): string {
    const firstPara = body
      .split(/\n\s*\n/)
      .map((s) => s.trim())
      .find((s) => s.length > 0) ?? "";
    return firstPara
      .replace(/^#{1,6}\s+/gm, "")
      .replace(/`([^`]+)`/g, "$1")
      .replace(/\*\*([^*]+)\*\*/g, "$1")
      .replace(/\*([^*]+)\*/g, "$1")
      .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
      .replace(/^[-*]\s+/gm, "")
      .replace(/\s+/g, " ")
      .trim();
  }

  const ownPrApprovalBlocked = $derived(github?.is_authored_by_me === true);
  const effectiveGithubUrl = $derived(github?.url ?? github_url ?? pr?.url ?? null);

  const canSubmit = $derived.by(() => ({
    comment: !submitting && reviewBody.trim().length > 0,
    approve: !submitting && !ownPrApprovalBlocked,
    changes: !submitting && reviewBody.trim().length > 0,
  }));

  async function submitAction(kind: "comment" | "approve" | "changes") {
    if (submitting) return;
    if (kind === "approve" && !canSubmit.approve) return;
    const body = reviewBody.trim();
    submitting = true;
    try {
      if (kind === "comment") {
        await app.cmd("post_github_pr_comment", { body });
      } else {
        const mode = kind === "approve" ? "APPROVE" : "REQUEST_CHANGES";
        await app.cmd("submit_github_pr_decision", { mode, summary: body });
      }
      reviewBody = "";
      actionsOpen = false;
    } finally {
      submitting = false;
    }
  }

  const watchStatus = $derived(app.snapshot?.watch_status ?? null);
  const isWatching = $derived(watchStatus?.active === true);
  const watchTitle = $derived(
    watchStatus?.active && watchStatus.branch && watchStatus.root_path
      ? `Watching ${watchStatus.branch} at ${watchStatus.root_path}`
      : "Watching",
  );
  let manualRefreshing = $state(false);
  const refreshing = $derived(manualRefreshing || (app.snapshot?.bg_loading?.gh_status ?? false));

  const checkStats = $derived.by(() => {
    if (!github) return { pass: 0, fail: 0, pending: 0, total: 0 };
    let pass = 0;
    let fail = 0;
    let pending = 0;
    for (const c of github.checks) {
      if (c.status === "PENDING") pending += 1;
      else if (c.conclusion === "SUCCESS" || c.conclusion === "pass") pass += 1;
      else if (c.conclusion === "FAILURE" || c.conclusion === "fail") fail += 1;
    }
    return { pass, fail, pending, total: github.checks.length };
  });

  function stateLabel(state: string, isDraft: boolean): { text: string; colorClass: string } {
    if (isDraft) return { text: "Draft", colorClass: "text-muted" };
    if (state === "OPEN") return { text: "Open", colorClass: "text-add-fg" };
    if (state === "MERGED") return { text: "Merged", colorClass: "text-periwinkle" };
    if (state === "CLOSED") return { text: "Closed", colorClass: "text-del-fg" };
    return { text: state, colorClass: "text-muted" };
  }

  function reviewLabel(decision: string | null): { text: string; colorClass: string } {
    if (decision === "APPROVED") return { text: "Approved", colorClass: "text-add-fg" };
    if (decision === "CHANGES_REQUESTED") return { text: "Changes req.", colorClass: "text-risk-med" };
    if (decision === "REVIEW_REQUIRED") return { text: "Required", colorClass: "text-del-fg" };
    return { text: "No review", colorClass: "text-muted" };
  }

  function mergeableLabel(m: string | null): { text: string; colorClass: string } | null {
    if (!m) return null;
    if (m === "MERGEABLE") return { text: "Yes", colorClass: "text-add-fg" };
    if (m === "CONFLICTING") return { text: "Conflicting", colorClass: "text-del-fg" };
    return { text: m.toLowerCase(), colorClass: "text-muted" };
  }

  async function onRefresh() {
    manualRefreshing = true;
    try {
      await app.cmd("refresh_github_status");
    } finally {
      manualRefreshing = false;
    }
  }

  async function onOpenPr() {
    if (effectiveGithubUrl) await app.cmd("open_url_in_browser", { url: effectiveGithubUrl });
  }

  const activeProject = $derived(app.snapshot?.projects?.find((p) => p.is_active) ?? null);
  const effectivePrNumber = $derived(
    pr_number ?? app.snapshot?.github?.number ?? pr?.number ?? null,
  );
  const isSaved = $derived(
    effectivePrNumber !== null &&
      (activeProject?.saved_prs ?? []).some((p) => p.number === effectivePrNumber),
  );

  async function toggleSaved() {
    if (!activeProject || effectivePrNumber === null) return;
    const title = github?.title ?? pr?.title ?? "";
    if (isSaved) {
      await app.cmd("unsave_pr", {
        projectId: activeProject.id,
        prNumber: effectivePrNumber,
      });
    } else {
      await app.cmd("save_pr", {
        projectId: activeProject.id,
        prNumber: effectivePrNumber,
        title,
      });
    }
  }

  const totalChanges = $derived(additions + deletions);

  const githubStateLabel = $derived(github ? stateLabel(github.state, github.is_draft) : null);
  const githubReviewLabel = $derived(github ? reviewLabel(github.review_decision) : null);
  const githubMergeLabel = $derived(github ? mergeableLabel(github.mergeable) : null);
</script>

<Card>
  <div class="flex flex-col gap-3.5">

    <!-- ── (a) Title row ──────────────────────────────────────────────────── -->
    <div class="flex items-center gap-1.5 min-w-0">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
        class="text-accent shrink-0">
        <line x1="6" y1="3" x2="6" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/>
        <path d="M18 9a9 9 0 0 1-9 9"/>
      </svg>
      <span class="text-[12px] font-medium text-fg truncate flex-1 min-w-0 font-mono">{branch}</span>
      <div class="flex items-center gap-1.5 shrink-0">
        {#if total_count > 0}
          <span class="text-[10px] text-muted whitespace-nowrap">{reviewed_count}/{total_count} reviewed</span>
        {/if}
        {#if isWatching}
          <span
            class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-full text-[10px] bg-add-bg text-add-fg border border-add-fg/30"
            title={watchTitle}
          >
            <span class="w-1.5 h-1.5 rounded-full bg-add-fg animate-pulse"></span>
            Live
          </span>
        {/if}
        {#if effectivePrNumber !== null && activeProject}
          <button
            type="button"
            aria-label={isSaved ? "Remove from saved" : "Save PR"}
            title={isSaved ? "Remove from saved" : "Save PR"}
            onclick={toggleSaved}
            class="{isSaved ? 'text-accent' : 'text-muted'} hover:text-fg-2"
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="{isSaved ? 'currentColor' : 'none'}" stroke="currentColor" stroke-width="2">
              <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/>
            </svg>
          </button>
        {/if}
      </div>
    </div>

    <!-- ── (b) Base ref row ───────────────────────────────────────────────── -->
    <div class="flex items-center gap-1.5 text-[11px] min-w-0">
      <span class="text-muted shrink-0">base</span>
      <span class="font-mono bg-bg border border-hairline rounded px-1.5 py-0.5 text-fg-2 truncate max-w-[80px]">{base}</span>
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-muted shrink-0">
        <line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 19 5 12 12 5"/>
      </svg>
      <span class="font-mono bg-bg border border-hairline rounded px-1.5 py-0.5 text-fg-2 truncate min-w-0 flex-1">{branch}</span>
    </div>

    <!-- ── (c) Changes meter ──────────────────────────────────────────────── -->
    <div class="flex flex-col gap-1.5">
      <div class="flex items-center gap-1.5 text-[11px]">
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-muted shrink-0">
          <polyline points="16 16 12 12 8 16"/><polyline points="8 8 12 12 16 8"/>
        </svg>
        <span class="text-fg-3">Changes</span>
        <span class="ml-auto font-mono">
          <span class="text-add-fg">+{additions.toLocaleString()}</span>
          <span class="text-fg-3 mx-0.5"> </span>
          <span class="text-del-fg">−{deletions.toLocaleString()}</span>
        </span>
      </div>
      {#if totalChanges > 0}
        <div class="h-1 flex rounded overflow-hidden gap-px">
          <span class="rounded-l" style="flex: {additions}; background: var(--color-add-fg); opacity: 0.7;"></span>
          <span class="rounded-r" style="flex: {deletions}; background: var(--color-del-fg); opacity: 0.7;"></span>
        </div>
      {:else}
        <div class="h-1 rounded bg-hairline"></div>
      {/if}
    </div>

    <!-- ── (d) GitHub card ────────────────────────────────────────────────── -->
    {#if github}
      <div class="border border-hairline rounded-lg overflow-hidden bg-bg">
        <!-- Header strip -->
        <div class="flex items-center gap-2 px-2.5 py-2 bg-card border-b border-hairline">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" class="shrink-0 text-fg-2">
            <path d="M12 0C5.4 0 0 5.4 0 12c0 5.3 3.4 9.8 8.2 11.4.6.1.8-.3.8-.6v-2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2 1-.3 2-.4 3-.4s2 .1 3 .4c2.3-1.6 3.3-1.2 3.3-1.2.7 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6C20.6 21.8 24 17.3 24 12c0-6.6-5.4-12-12-12z"/>
          </svg>
          <button
            type="button"
            class="text-[12px] font-medium text-periwinkle hover:text-periwinkle/80"
            onclick={onOpenPr}
            title="Open PR #{github.number} in browser"
          >#{github.number}</button>
          <div class="flex-1"></div>
          <button
            type="button"
            class={`p-1 rounded transition-colors ${refreshing ? "text-muted opacity-50 cursor-not-allowed" : "text-muted hover:text-fg-2 hover:bg-fg-3/10"}`}
            onclick={onRefresh}
            disabled={refreshing}
            title="Refresh GitHub status"
            aria-label="Refresh GitHub status"
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
              class:animate-spin={refreshing}>
              <path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>
            </svg>
          </button>
        </div>

        <!-- Body -->
        <div class="p-2.5 flex flex-col gap-2">
          <!-- 3-cell status grid -->
          <div class="grid grid-cols-3 gap-1.5">
            <!-- Status cell -->
            <div class="bg-card rounded-md p-1.5 flex flex-col gap-1">
              <div class="text-[9px] uppercase tracking-wider text-muted font-semibold flex items-center gap-1">
                <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M13 6h3a2 2 0 0 1 2 2v7"/><line x1="6" y1="9" x2="6" y2="21"/></svg>
                Status
              </div>
              {#if githubStateLabel}
                <div class="text-[11px] font-medium {githubStateLabel.colorClass}">{githubStateLabel.text}</div>
              {/if}
            </div>
            <!-- Review cell -->
            <div class="bg-card rounded-md p-1.5 flex flex-col gap-1">
              <div class="text-[9px] uppercase tracking-wider text-muted font-semibold flex items-center gap-1">
                <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
                Review
              </div>
              {#if githubReviewLabel}
                <div class="text-[11px] font-medium {githubReviewLabel.colorClass}">{githubReviewLabel.text}</div>
              {/if}
            </div>
            <!-- Mergeable cell -->
            <div class="bg-card rounded-md p-1.5 flex flex-col gap-1">
              <div class="text-[9px] uppercase tracking-wider text-muted font-semibold flex items-center gap-1">
                <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 9v6a3 3 0 0 0 3 3h9"/></svg>
                Merge
              </div>
              <div class="text-[11px] font-medium {githubMergeLabel ? githubMergeLabel.colorClass : 'text-muted'}">{githubMergeLabel ? githubMergeLabel.text : "—"}</div>
            </div>
          </div>

          <!-- CI row -->
          {#if checkStats.total > 0}
            <button
              type="button"
              class="flex items-center gap-2 text-[12px] bg-card rounded-md px-2 py-1.5 hover:bg-hover w-full text-left transition-colors"
              onclick={() => (expandChecks = !expandChecks)}
              data-testid="checks-toggle"
            >
              {#if checkStats.fail === 0 && checkStats.pending === 0}
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-add-fg shrink-0">
                  <path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/>
                </svg>
              {:else if checkStats.fail > 0}
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-del-fg shrink-0">
                  <circle cx="12" cy="12" r="10"/><path d="M15 9l-6 6M9 9l6 6"/>
                </svg>
              {:else}
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-risk-med shrink-0">
                  <circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>
                </svg>
              {/if}
              <span
                class:text-add-fg={checkStats.fail === 0 && checkStats.pending === 0}
                class:text-del-fg={checkStats.fail > 0}
                class:text-risk-med={checkStats.pending > 0 && checkStats.fail === 0}
                class="font-medium"
              >{checkStats.pass}/{checkStats.total}</span>
              <span class="text-fg-3">checks passing</span>
              {#if checkStats.fail > 0}
                <span class="text-del-fg ml-1">· {checkStats.fail} failing</span>
              {/if}
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                class="ml-auto transition-transform text-muted"
                class:rotate-180={expandChecks}><path d="M6 9l6 6 6-6"/></svg>
            </button>
            {#if expandChecks}
              <ul class="space-y-1 pl-1 border-l border-hairline ml-1">
                {#each github.checks as c}
                  <li class="flex items-center gap-2 text-[11px] pl-2">
                    <span
                      class="inline-block w-1.5 h-1.5 rounded-full shrink-0"
                      class:bg-add-fg={c.conclusion === "SUCCESS" || c.conclusion === "pass"}
                      class:bg-del-fg={c.conclusion === "FAILURE" || c.conclusion === "fail"}
                      class:bg-risk-med={c.status === "PENDING"}
                      class:bg-fg-3={c.status === "COMPLETED" &&
                        c.conclusion !== "SUCCESS" &&
                        c.conclusion !== "FAILURE" &&
                        c.conclusion !== "pass" &&
                        c.conclusion !== "fail"}
                    ></span>
                    {#if c.url}
                      <button
                        type="button"
                        class="text-fg-2 hover:text-fg-1 hover:underline truncate text-left"
                        onclick={() => openExternalUrl(c.url!)}
                      >{c.name}</button>
                    {:else}
                      <span class="text-fg-2 truncate">{c.name}</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          {/if}

          <!-- Labels -->
          {#if github.labels.length > 0}
            <div class="flex flex-wrap gap-1">
              {#each github.labels as label}
                <span class="text-[10px] px-1.5 py-0.5 rounded border border-hairline text-fg-3">{label}</span>
              {/each}
            </div>
          {/if}

          <!-- Activity counts -->
          <div class="flex items-center gap-3 text-[11px] text-muted">
            <span>{github.comments_count} comment{github.comments_count === 1 ? "" : "s"}</span>
            <span>·</span>
            <span>{github.reviews_count} review{github.reviews_count === 1 ? "" : "s"}</span>
          </div>
        </div>
      </div>

    {:else}
      <!-- Fallback: simple PR + checks rows when no live GitHub data -->
      {#if effectivePrNumber !== null}
        {#if effectiveGithubUrl}
          <button
            class="w-full flex items-center gap-2 hover:bg-hover rounded px-1.5 py-1 -mx-1 transition-colors"
            onclick={onOpenPr}
            aria-label="Open PR #{effectivePrNumber} on GitHub"
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 9v6a3 3 0 0 0 3 3h9"/></svg>
            <span class="text-fg-2 text-[12px]">PR #{effectivePrNumber}</span>
            {#if pr?.state}
              <span class="ml-auto text-muted text-[11px]">{pr.state}</span>
            {/if}
          </button>
        {:else}
          <div class="flex items-center gap-2 px-1.5">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 9v6a3 3 0 0 0 3 3h9"/></svg>
            <span class="text-fg-2 text-[12px]">PR #{effectivePrNumber}</span>
            {#if pr?.state}
              <span class="ml-auto text-muted text-[11px]">{pr.state}</span>
            {/if}
          </div>
        {/if}
      {/if}
      {#if checks_status === "success"}
        <div class="flex items-center gap-2 px-1.5">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#7ee2a8" stroke-width="2"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
          <span class="text-fg-2 text-[12px]">Checks successful</span>
          {#if github_url}
            <button class="ml-auto text-muted hover:text-fg-2" onclick={() => app.cmd("open_url_in_browser", { url: github_url })} aria-label="View checks on GitHub">
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
            </button>
          {/if}
        </div>
      {:else if checks_status === "pending"}
        <div class="flex items-center gap-2 px-1.5">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#fbbf24" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg>
          <span class="text-fg-2 text-[12px]">Checks pending</span>
          {#if github_url}
            <button class="ml-auto text-muted hover:text-fg-2" onclick={() => app.cmd("open_url_in_browser", { url: github_url })} aria-label="View checks on GitHub">
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
            </button>
          {/if}
        </div>
      {:else if checks_status === "failure"}
        <div class="flex items-center gap-2 px-1.5">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#f4a3a3" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M15 9l-6 6M9 9l6 6"/></svg>
          <span class="text-fg-2 text-[12px]">Checks failing</span>
          {#if github_url}
            <button class="ml-auto text-muted hover:text-fg-2" onclick={() => app.cmd("open_url_in_browser", { url: github_url })} aria-label="View checks on GitHub">
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
            </button>
          {/if}
        </div>
      {/if}
    {/if}

    <!-- ── (e) Description block ──────────────────────────────────────────── -->
    {#if github?.body.trim()}
      <div>
        <div class="text-[10px] uppercase tracking-wider text-muted font-semibold mb-1.5">Description</div>
        {#if descriptionOpen}
          <div class="text-[12px] text-fg-2 leading-relaxed mb-1.5">
            <MarkdownText text={github.body} />
          </div>
        {:else}
          <div class="text-[12px] text-fg-2 leading-relaxed mb-1.5 line-clamp-2">
            {descriptionPreview(github.body)}
          </div>
        {/if}
        <button
          type="button"
          onclick={() => (descriptionOpen = !descriptionOpen)}
          class="text-[11px] text-periwinkle hover:text-periwinkle/80"
        >{descriptionOpen ? "Hide" : "Show all"}</button>
      </div>
    {/if}

    <!-- ── (f) Composer button ─────────────────────────────────────────────── -->
    {#if github}
      <div class="border-t border-hairline pt-3">
        {#if !actionsOpen}
          <button
            type="button"
            onclick={() => (actionsOpen = true)}
            class="w-full flex items-center gap-2 px-2.5 py-2 rounded-md bg-bg border border-hairline text-[12px] text-muted hover:text-fg-2 hover:border-border transition-colors text-left"
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/>
            </svg>
            Comment or review…
          </button>
        {:else}
          <textarea
            bind:value={reviewBody}
            placeholder="Leave a PR-wide comment. Required for Comment and Request changes; optional for Approve."
            rows="3"
            class="w-full text-[12px] bg-bg border border-hairline rounded px-2 py-1.5 focus:border-accent outline-none resize-y placeholder:text-muted"
            disabled={submitting}
          ></textarea>
          <div class="flex items-center gap-1.5 mt-2 flex-wrap">
            <button
              type="button"
              onclick={() => submitAction("comment")}
              disabled={!canSubmit.comment}
              class="px-2 py-1 rounded text-[11px] font-medium bg-accent text-black hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
              title="Post a PR-wide comment"
            >Comment</button>
            <button
              type="button"
              onclick={() => submitAction("approve")}
              disabled={!canSubmit.approve}
              class="px-2 py-1 rounded text-[11px] font-medium bg-add-fg text-black hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
              title={ownPrApprovalBlocked ? "GitHub does not allow approving your own PR" : "Submit an approving review"}
            >Approve</button>
            <button
              type="button"
              onclick={() => submitAction("changes")}
              disabled={!canSubmit.changes}
              class="px-2 py-1 rounded text-[11px] font-medium bg-del-fg text-black hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
              title="Submit a Request changes review"
            >Request changes</button>
            <button
              type="button"
              onclick={() => { actionsOpen = false; reviewBody = ""; }}
              disabled={submitting}
              class="ml-auto text-[11px] text-muted hover:text-fg-2 px-2 py-1 rounded disabled:opacity-50"
            >Cancel</button>
          </div>
          {#if ownPrApprovalBlocked}
            <div class="mt-1.5 text-[10px] text-muted">GitHub does not allow approving your own PR.</div>
          {/if}
          {#if submitting}
            <div class="mt-1.5 text-[10px] text-muted">Submitting…</div>
          {/if}
        {/if}
      </div>
    {/if}

  </div>
</Card>
