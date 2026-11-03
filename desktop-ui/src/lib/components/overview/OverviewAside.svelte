<script lang="ts">
  import type { AppSnapshot, GhReviewerSummary } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { reviewPage } from "$lib/stores/reviewPage.svelte";
  import { openOverviewFile } from "$lib/overviewFileOpen";
  import { rightRail } from "$lib/stores/rightRail.svelte";
  import { rightPanelTab } from "$lib/stores/rightPanelTab.svelte";
  import { rightPanelFilters } from "$lib/components/rpanel/panelFilters.svelte";
  import { commentsAuthorFilter } from "$lib/components/rpanel/commentsAuthorFilter.svelte";
  import { reviewPageTabKey } from "$lib/stores/reviewPageModel";
  import { githubStatusForActiveTab, resolveActivePrNumber, resolveActivePrUrl } from "$lib/prUrl";
  import { resolveContextIdentity } from "$lib/contextIdentity";
  import { openExternalUrl } from "$lib/openExternalUrl";
  import { avatarToken, initials } from "$lib/activityTimeline";
  import type { MergeBoxTone } from "$lib/prMergeBox";
  import PrMergeBox from "$lib/components/PrMergeBox.svelte";
  import BranchCard from "$lib/components/BranchCard.svelte";
  import CommentsCard from "$lib/components/CommentsCard.svelte";
  import ReviewerCommentsPopover from "./ReviewerCommentsPopover.svelte";
  import { asideFileGroups, checkRows, prStatus, type CheckState } from "./overviewModel";
  import { overviewSectionId } from "./overviewNav";
  import OverviewSection from "./OverviewSection.svelte";

  interface Props {
    /** Defaults to the live snapshot; stories pass one. */
    snapshot?: AppSnapshot | null;
  }

  const { snapshot: snapshotOverride }: Props = $props();
  const snap = $derived(snapshotOverride !== undefined ? snapshotOverride : app.snapshot);

  const github = $derived(snap ? githubStatusForActiveTab(snap) : null);
  const ai = $derived(snap?.ai ?? null);
  const status = $derived(github ? prStatus(github) : null);
  const checks = $derived(checkRows(github?.checks ?? []));
  const reviewers = $derived(github?.reviewers ?? []);
  const groups = $derived(asideFileGroups(snap));
  const fileCount = $derived(groups.reduce((n, g) => n + g.files.length, 0));
  const reviewed = $derived(snap?.reviewed_count ?? 0);
  const total = $derived(snap?.total_count ?? 0);

  const STATUS_TEXT: Record<MergeBoxTone, string> = {
    ok: "text-add-fg",
    warn: "text-warning",
    danger: "text-del-fg",
    muted: "text-fg-2",
    merged: "text-question",
  };
  const CHECK_DOT: Record<CheckState, string> = {
    ok: "bg-add-fg",
    pend: "bg-warning",
    fail: "bg-del-fg",
    neutral: "bg-fg-3",
  };

  // ── Branch & PR (the right panel's old Branch tab) ────────────────────────
  const identity = $derived(resolveContextIdentity(snap));
  const tabOwnsPr = $derived(snap?.tabs.find((t) => t.is_active)?.pr_number != null);
  const isMerged = $derived(
    github?.state === "MERGED" ||
      (!tabOwnsPr && (snap?.worktrees.find((w) => w.is_current)?.is_merged ?? false)),
  );
  const checksStatus = $derived.by((): "success" | "pending" | "failure" | null => {
    if (checks.length === 0) return null;
    if (checks.some((c) => c.state === "fail")) return "failure";
    if (checks.some((c) => c.state === "pend")) return "pending";
    return "success";
  });

  const avatarStyle = (name: string) => {
    const color = `var(${avatarToken(name)})`;
    return `color: ${color}; background: color-mix(in srgb, ${color} 18%, transparent)`;
  };

  // The popover's own default opens the Comments tab; this also narrows it to
  // the reviewer, which is what the button promises.
  function showInPanel(reviewer: GhReviewerSummary) {
    commentsAuthorFilter.set(reviewer.reviewer, reviewPageTabKey(snap ?? null));
    rightPanelFilters.setKind("comment");
    rightPanelTab.set("comments");
    rightRail.expand();
    reviewPage.show("diff");
  }

  const h3 = "m-0 mb-2 text-[11px] font-medium uppercase tracking-[.06em] text-fg-3";
</script>

{#snippet avatar(name: string)}
  <span
    class="inline-grid h-[18px] w-[18px] shrink-0 place-items-center rounded-full text-[9px] font-semibold"
    style={avatarStyle(name)}
    aria-hidden="true"
  >{initials(name)}</span>
{/snippet}

<aside class="grid content-start gap-6 min-w-0" aria-label="Pull request details" data-testid="overview-aside">
  {#if github && status}
    <OverviewSection name="aside-status" title="Status">
      <h3 class={h3}>Status</h3>
      <span class="inline-flex items-center gap-1.5 text-[13px] font-medium {STATUS_TEXT[status.tone]}">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><circle cx="4" cy="3.5" r="1.8" /><circle cx="4" cy="12.5" r="1.8" /><circle cx="12" cy="12.5" r="1.8" /><path d="M4 5.3v5.4M12 10.7V7a3 3 0 00-3-3H7" /></svg>
        {status.label}
      </span>
      <div class="mt-2.5">
        <PrMergeBox {github} />
      </div>
    </OverviewSection>
  {/if}

  <OverviewSection name="aside-reviewers" title="Reviewers">
    <h3 class={h3}>Reviewers</h3>
    <div class="grid gap-1.5 text-[12.5px]">
      <div class="flex min-w-0 items-center gap-2">
        {@render avatar("You")}
        <span class="truncate">You</span>
        <span class="ml-auto shrink-0 text-[11px] {total > 0 && reviewed === total ? 'text-add-fg' : 'text-fg-3'}">
          {total > 0 && reviewed === total ? "reviewed all" : `${reviewed}/${total} reviewed`}
        </span>
      </div>
      {#each reviewers as reviewer (reviewer.reviewer)}
        <div class="flex min-w-0 items-center gap-2">
          {@render avatar(reviewer.reviewer)}
          <span class="truncate" title={reviewer.reviewer}>{reviewer.reviewer}</span>
          <ReviewerCommentsPopover
            {reviewer}
            reviews={github?.recent_reviews ?? []}
            comments={github?.recent_comments ?? []}
            threads={ai?.threads ?? []}
            onShowInPanel={showInPanel}
          />
        </div>
      {/each}
    </div>
  </OverviewSection>

  <!-- Branch tools the right panel's Branch tab held: stack, save, watch and
       the GitHub summary, then pushing and submitting comments. Open by
       default and right under who is reviewing: it is where the PR is acted on. -->
  {#if snap}
    <OverviewSection id={overviewSectionId("branch")} name="aside-branch" title="Branch & PR" class="scroll-mt-4">
      <h3 class={h3}>Branch &amp; PR</h3>
      <div class="grid gap-3">
        <BranchCard
          branch={identity.branch}
          base={identity.base}
          pr={snap.pr}
          reviewed_count={snap.reviewed_count}
          total_count={snap.total_count}
          additions={snap.diff_stats?.total.additions ?? 0}
          deletions={snap.diff_stats?.total.deletions ?? 0}
          checks_status={checksStatus}
          pr_number={resolveActivePrNumber(snap)}
          is_merged={isMerged}
          github_url={resolveActivePrUrl(snap)}
          {github}
          mergeBox={false}
        />
        {#if ai}
          <CommentsCard {ai} active={false} />
        {/if}
      </div>
    </OverviewSection>
  {/if}

  {#if checks.length > 0}
    <OverviewSection name="aside-checks" title="Checks">
      <h3 class={h3}>Checks <span class="normal-case tracking-normal text-fg-3">{checks.length}</span></h3>
      <!-- Capped so a monorepo's forty checks do not push Files changed off screen. -->
      <div class="-mr-1.5 grid max-h-[220px] gap-1.5 overflow-y-auto pr-1.5">
        {#each checks as check, i (`${check.name}:${i}`)}
          <div class="flex min-w-0 items-center gap-2 text-[12px]">
            <span class="h-[7px] w-[7px] shrink-0 rounded-full {CHECK_DOT[check.state]}" aria-hidden="true"></span>
            {#if check.url}
              <button
                type="button"
                class="min-w-0 truncate text-left hover:underline"
                title="Open {check.name}"
                onclick={() => check.url && void openExternalUrl(check.url)}
              >{check.name}</button>
            {:else}
              <span class="min-w-0 truncate">{check.name}</span>
            {/if}
            <span class="mono ml-auto shrink-0 text-[11px] text-fg-3">{check.detail}</span>
          </div>
        {/each}
      </div>
    </OverviewSection>
  {/if}

  {#if groups.length > 0}
    <OverviewSection name="aside-files" title="Files changed">
      <h3 class={h3}>Files changed <span class="normal-case tracking-normal text-fg-3">{fileCount}</span></h3>
      <div class="-mx-1.5 grid max-h-[320px] content-start gap-0.5 overflow-y-auto px-1.5">
        {#each groups as group, gi (group.title ?? gi)}
          {#if group.title}
            <div class="mb-0.5 flex justify-between gap-2 text-[12px] text-fg-2 {gi > 0 ? 'mt-2' : ''}">
              <span class="min-w-0 truncate">{group.title}</span>
              <span class="mono shrink-0 text-[11px]"><span class="text-add-fg">+{group.additions}</span> <span class="text-del-fg">−{group.deletions}</span></span>
            </div>
          {/if}
          {#each group.files as file (file.path)}
            <button
              type="button"
              class="-mx-1.5 grid grid-cols-[26px_minmax(0,1fr)_auto] items-center gap-2 rounded-[5px] px-1.5 py-[3px] text-left text-[12px] hover:bg-hover"
              title="{file.path} · ⌘-click to open the source"
              onclick={(e) => void openOverviewFile(file.path, e)}
            >
              <span class="mono rounded border border-hairline text-center text-[9px] leading-[14px] text-fg-3">{file.lang}</span>
              <span class="truncate {file.reviewed ? 'text-fg-3 line-through' : ''}">{file.name}</span>
              <span class="mono text-[10.5px] text-fg-3">{file.renamed ? "R" : ""}</span>
            </button>
          {/each}
        {/each}
      </div>
    </OverviewSection>
  {/if}
</aside>
