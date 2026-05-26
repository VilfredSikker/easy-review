<script lang="ts">
  import type { PrSnapshot, GithubStatusSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
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

  // First non-empty line of the body, stripped of obvious markdown syntax,
  // used for the collapsed preview. Full markdown renders when expanded.
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
  let actionsOpen = $state(false);
  let reviewBody = $state("");
  let submitting = $state(false);
  const ownPrApprovalBlocked = $derived(github?.is_authored_by_me === true);
  const effectiveGithubUrl = $derived(github?.url ?? github_url ?? pr?.url ?? null);

  // GitHub itself rejects REQUEST_CHANGES with an empty body and a plain comment
  // needs a body too — gate locally so we don't round-trip just to fail.
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
        // Use the decision-only command so pending inline drafts (which may have
        // stale line anchors) aren't bundled and 422'd by GitHub.
        const mode = kind === "approve" ? "APPROVE" : "REQUEST_CHANGES";
        await app.cmd("submit_github_pr_decision", { mode, summary: body });
      }
      // Errors surface via the global toast (app.cmd swallows + showsToast); close
      // the drawer either way so the user sees the updated state on next refresh.
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

  function reviewLabel(decision: string | null): { text: string; tone: string } {
    if (decision === "APPROVED") return { text: "Approved", tone: "ok" };
    if (decision === "CHANGES_REQUESTED") return { text: "Changes requested", tone: "warn" };
    if (decision === "REVIEW_REQUIRED") return { text: "Review required", tone: "info" };
    return { text: "No decision", tone: "muted" };
  }

  function mergeableLabel(m: string | null): { text: string; tone: string } | null {
    if (!m) return null;
    if (m === "MERGEABLE") return { text: "Mergeable", tone: "ok" };
    if (m === "CONFLICTING") return { text: "Conflicting", tone: "bad" };
    return { text: m.toLowerCase(), tone: "muted" };
  }

  function stateLabel(state: string, isDraft: boolean): { text: string; tone: string } {
    if (isDraft) return { text: "Draft", tone: "muted" };
    if (state === "OPEN") return { text: "Open", tone: "ok" };
    if (state === "MERGED") return { text: "Merged", tone: "info" };
    if (state === "CLOSED") return { text: "Closed", tone: "bad" };
    return { text: state, tone: "muted" };
  }

  function toneClass(tone: string): string {
    switch (tone) {
      case "ok":
        return "bg-add-bg text-add-fg border-add-fg/30";
      case "warn":
        return "bg-card text-risk-med border-risk-med/30";
      case "bad":
        return "bg-del-bg text-del-fg border-del-fg/30";
      case "info":
        return "bg-accent-soft text-accent border-accent-border";
      default:
        return "bg-card text-fg-3 border-hairline";
    }
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
</script>

<Card>
  <div class="flex items-center justify-between mb-3">
    <SectionLabel>Branch</SectionLabel>
    {#if effectivePrNumber !== null && activeProject}
      <button
        type="button"
        aria-label={isSaved ? "Remove from saved" : "Save PR"}
        title={isSaved ? "Remove from saved" : "Save PR"}
        onclick={toggleSaved}
        class="{isSaved ? 'text-accent' : 'text-muted'} hover:text-fg-2"
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="{isSaved ? 'currentColor' : 'none'}" stroke="currentColor" stroke-width="2">
          <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/>
        </svg>
      </button>
    {/if}
  </div>
  <div class="text-sm mono mb-3 truncate text-fg-2 flex items-center gap-1.5">
    <span class="truncate">{branch}</span>
    {#if total_count > 0}
      <span class="shrink-0 px-1.5 py-0.5 rounded-full text-[10px] font-sans bg-bg text-muted">
        {reviewed_count}/{total_count} reviewed
      </span>
    {/if}
    {#if isWatching}
      <span
        class="shrink-0 inline-flex items-center gap-1 px-1.5 py-0.5 rounded-full text-[10px] font-sans bg-add-bg text-add-fg border border-add-fg/30"
        title={watchTitle}
      >
        <span class="w-1.5 h-1.5 rounded-full bg-add-fg animate-pulse"></span>
        Watching
      </span>
    {/if}
    {#if is_pr && !github}
      <span title={is_merged ? "Merged PR" : "Pull request"} class="inline-flex items-center gap-1 shrink-0 {is_merged ? 'text-purple-400' : 'text-accent'}">
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M13 6h3a2 2 0 0 1 2 2v7"/><line x1="6" y1="9" x2="6" y2="21"/></svg>
        {#if pr_number !== null}
          <span class="text-[10px] font-mono">#{pr_number}</span>
        {/if}
      </span>
    {/if}
  </div>
  <div class="text-xs text-fg-3 mb-3 mono">{base} ← <span class="text-fg-2">{branch}</span></div>

  <div class="space-y-2 text-sm">
    <!-- Changes row -->
    <div class="flex items-center gap-2">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2"><polyline points="16 16 12 12 8 16"/><line x1="12" y1="12" x2="12" y2="21"/><path d="M20.39 18.39A5 5 0 0 0 18 9h-1.26A8 8 0 1 0 3 16.3"/></svg>
      <span class="text-fg-2">Changes</span>
      <span class="ml-auto mono text-xs"><span class="text-add-fg">+{additions.toLocaleString()}</span> <span class="text-del-fg">−{deletions.toLocaleString()}</span></span>
    </div>

    {#if github}
      <!-- GitHub status block -->
      <div class="space-y-2 pt-1 border-t border-hairline text-sm text-fg-2">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2 font-medium text-fg-1">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"
              ><path
                d="M12 0C5.4 0 0 5.4 0 12c0 5.3 3.4 9.8 8.2 11.4.6.1.8-.3.8-.6v-2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2 1-.3 2-.4 3-.4s2 .1 3 .4c2.3-1.6 3.3-1.2 3.3-1.2.7 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6C20.6 21.8 24 17.3 24 12c0-6.6-5.4-12-12-12z"
              /></svg
            >
            <span>GitHub</span>
            <button
              type="button"
              class="text-accent hover:text-accent/80 underline underline-offset-2 font-mono text-xs"
              onclick={onOpenPr}
              title="Open PR #{github.number} in browser"
            >
              #{github.number}
            </button>
          </div>
          <button
            type="button"
            class={`text-xs px-1.5 py-0.5 rounded transition-colors ${refreshing ? "text-fg-3 opacity-50 cursor-not-allowed" : "text-fg-3 hover:text-fg-1 hover:bg-fg-3/10"}`}
            onclick={onRefresh}
            disabled={refreshing}
            title="Refresh GitHub status"
            aria-label="Refresh GitHub status"
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              class:animate-spin={refreshing}
            ><path d="M21 12a9 9 0 1 1-3-6.7L21 8" /><path d="M21 3v5h-5" /></svg>
          </button>
        </div>

        <div class="pl-5 space-y-2">
          <!-- Chip row -->
          <div class="flex flex-wrap items-center gap-1.5">
            {#each [stateLabel(github.state, github.is_draft)] as s}
              <span class="text-[10px] px-1.5 py-0.5 rounded border {toneClass(s.tone)}">{s.text}</span>
            {/each}
            {#each [reviewLabel(github.review_decision)] as r}
              <span class="text-[10px] px-1.5 py-0.5 rounded border {toneClass(r.tone)}">{r.text}</span>
            {/each}
            {#if mergeableLabel(github.mergeable)}
              {@const m = mergeableLabel(github.mergeable)!}
              <span class="text-[10px] px-1.5 py-0.5 rounded border {toneClass(m.tone)}">{m.text}</span>
            {/if}
          </div>

          <!-- PR description (collapsible — mirrors AiReviewCard summary) -->
          {#if github.body.trim()}
            <div>
              {#if descriptionOpen}
                <div class="text-sm text-fg-2 leading-relaxed mb-1.5">
                  <MarkdownText text={github.body} />
                </div>
              {:else}
                <div class="text-sm text-fg-2 leading-relaxed mb-1.5 line-clamp-2">
                  {descriptionPreview(github.body)}
                </div>
              {/if}
              <button
                type="button"
                onclick={() => (descriptionOpen = !descriptionOpen)}
                class="flex items-center gap-1 text-xs text-fg-3 hover:text-fg-1"
              >
                {descriptionOpen ? "Hide description" : "Show description"}
                <svg
                  width="10"
                  height="10"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  class="transition-transform"
                  class:rotate-180={descriptionOpen}><path d="M6 9l6 6 6-6" /></svg>
              </button>
            </div>
          {/if}

          <!-- Checks summary -->
          {#if checkStats.total > 0}
            <button
              type="button"
              class="flex items-center gap-2 text-xs hover:text-fg-1"
              onclick={() => (expandChecks = !expandChecks)}
              data-testid="checks-toggle"
            >
              <span
                class:text-add-fg={checkStats.fail === 0 && checkStats.pending === 0}
                class:text-del-fg={checkStats.fail > 0}
                class:text-risk-med={checkStats.pending > 0 && checkStats.fail === 0}
              >
                {checkStats.pass}/{checkStats.total} passing
              </span>
              {#if checkStats.fail > 0}
                <span class="text-del-fg">· {checkStats.fail} failing</span>
              {/if}
              {#if checkStats.pending > 0}
                <span class="text-risk-med">· {checkStats.pending} pending</span>
              {/if}
              <svg
                width="10"
                height="10"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="transition-transform"
                class:rotate-180={expandChecks}><path d="M6 9l6 6 6-6" /></svg
              >
            </button>
            {#if expandChecks}
              <ul class="space-y-1 pl-1 border-l border-hairline">
                {#each github.checks as c}
                  <li class="flex items-center gap-2 text-xs pl-2">
                    <span
                      class="inline-block w-1.5 h-1.5 rounded-full"
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

          <!-- Counts -->
          <div class="flex items-center gap-3 text-xs text-fg-3">
            <span>{github.comments_count} comment{github.comments_count === 1 ? "" : "s"}</span>
            <span>{github.reviews_count} review{github.reviews_count === 1 ? "" : "s"}</span>
          </div>

          <!-- PR-wide comment + review actions -->
          <div class="pt-2 mt-1 border-t border-hairline">
            {#if !actionsOpen}
              <button
                type="button"
                onclick={() => (actionsOpen = true)}
                class="w-full text-left text-xs text-fg-3 hover:text-fg-1 flex items-center gap-1"
              >
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/></svg>
                Comment or review…
              </button>
            {:else}
              <textarea
                bind:value={reviewBody}
                placeholder="Leave a PR-wide comment. Required for Comment and Request changes; optional for Approve."
                rows="3"
                class="w-full text-sm bg-bg border border-hairline rounded px-2 py-1.5 focus:border-accent outline-none resize-y placeholder:text-muted"
                disabled={submitting}
              ></textarea>
              <div class="flex items-center gap-1.5 mt-2 flex-wrap">
                <button
                  type="button"
                  onclick={() => submitAction("comment")}
                  disabled={!canSubmit.comment}
                  class="px-2 py-1 rounded text-[11px] font-medium bg-accent text-black hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Post a PR-wide comment (issue conversation stream)"
                >Comment</button>
                <button
                  type="button"
                  onclick={() => submitAction("approve")}
                  disabled={!canSubmit.approve}
                  class="px-2 py-1 rounded text-[11px] font-medium bg-add-fg text-black hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
                  title={ownPrApprovalBlocked ? "GitHub does not allow approving your own PR" : "Submit an approving review (body optional)"}
                >Approve</button>
                <button
                  type="button"
                  onclick={() => submitAction("changes")}
                  disabled={!canSubmit.changes}
                  class="px-2 py-1 rounded text-[11px] font-medium bg-del-fg text-black hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Submit a Request changes review (body required by GitHub)"
                >Request changes</button>
                <button
                  type="button"
                  onclick={() => { actionsOpen = false; reviewBody = ""; }}
                  disabled={submitting}
                  class="ml-auto text-[11px] text-fg-3 hover:text-fg-1 px-2 py-1 rounded disabled:opacity-50"
                >Cancel</button>
              </div>
              {#if ownPrApprovalBlocked}
                <div class="mt-1.5 text-[10px] text-fg-3">GitHub does not allow approving your own PR.</div>
              {/if}
              {#if submitting}
                <div class="mt-1.5 text-[10px] text-fg-3">Submitting…</div>
              {/if}
            {/if}
          </div>
        </div>
      </div>
    {:else}
      <!-- Fallback: simple PR and checks rows when no live GitHub data -->
      {#if effectivePrNumber !== null}
        {#if effectiveGithubUrl}
          <button
            class="w-full flex items-center gap-2 hover:bg-ink-700/40 rounded px-1 -mx-1 transition-colors"
            onclick={onOpenPr}
            aria-label="Open PR #{effectivePrNumber} on GitHub"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 9v6a3 3 0 0 0 3 3h9"/></svg>
            <span class="text-fg-2">PR #{effectivePrNumber}</span>
            {#if pr?.state}
              <span class="ml-auto text-muted text-xs">{pr.state}</span>
            {/if}
          </button>
        {:else}
          <div class="flex items-center gap-2">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 9v6a3 3 0 0 0 3 3h9"/></svg>
            <span class="text-fg-2">PR #{effectivePrNumber}</span>
            {#if pr?.state}
              <span class="ml-auto text-muted text-xs">{pr.state}</span>
            {/if}
          </div>
        {/if}
      {/if}
      {#if checks_status === "success"}
        {#if github_url}
          <button
            class="w-full flex items-center gap-2 hover:bg-ink-700/40 rounded px-1 -mx-1 transition-colors"
            onclick={() => app.cmd("open_url_in_browser", { url: github_url })}
            aria-label="View checks on GitHub"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#7ee2a8" stroke-width="2"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
            <span class="text-fg-2">Checks successful</span>
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#5e5e5e" stroke-width="2" class="ml-auto"><polyline points="9 18 15 12 9 6"/></svg>
          </button>
        {:else}
          <div class="flex items-center gap-2">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#7ee2a8" stroke-width="2"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
            <span class="text-fg-2">Checks successful</span>
          </div>
        {/if}
      {:else if checks_status === "pending"}
        {#if github_url}
          <button
            class="w-full flex items-center gap-2 hover:bg-ink-700/40 rounded px-1 -mx-1 transition-colors"
            onclick={() => app.cmd("open_url_in_browser", { url: github_url })}
            aria-label="View checks on GitHub"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#fbbf24" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg>
            <span class="text-fg-2">Checks pending</span>
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#5e5e5e" stroke-width="2" class="ml-auto"><polyline points="9 18 15 12 9 6"/></svg>
          </button>
        {:else}
          <div class="flex items-center gap-2">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#fbbf24" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg>
            <span class="text-fg-2">Checks pending</span>
          </div>
        {/if}
      {:else if checks_status === "failure"}
        {#if github_url}
          <button
            class="w-full flex items-center gap-2 hover:bg-ink-700/40 rounded px-1 -mx-1 transition-colors"
            onclick={() => app.cmd("open_url_in_browser", { url: github_url })}
            aria-label="View checks on GitHub"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#f4a3a3" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M15 9l-6 6M9 9l6 6"/></svg>
            <span class="text-fg-2">Checks failing</span>
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#5e5e5e" stroke-width="2" class="ml-auto"><polyline points="9 18 15 12 9 6"/></svg>
          </button>
        {:else}
          <div class="flex items-center gap-2">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#f4a3a3" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M15 9l-6 6M9 9l6 6"/></svg>
            <span class="text-fg-2">Checks failing</span>
          </div>
        {/if}
      {/if}
    {/if}
  </div>
</Card>
