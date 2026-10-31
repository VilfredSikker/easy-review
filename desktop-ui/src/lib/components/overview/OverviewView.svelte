<script lang="ts">
  import type { AppSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { githubStatusForActiveTab } from "$lib/prUrl";
  import { extractPrLinks } from "$lib/prLinks";
  import { longAge } from "$lib/relativeTime";
  import { avatarToken, initials } from "$lib/activityTimeline";
  import TriageCard from "$lib/components/TriageCard.svelte";
  import ChecklistCard from "$lib/components/ChecklistCard.svelte";
  import FileRisksCard from "$lib/components/FileRisksCard.svelte";
  import DiagramsCard from "$lib/components/DiagramsCard.svelte";
  import AgentOutputCard from "$lib/components/AgentOutputCard.svelte";
  import QuickActions from "./QuickActions.svelte";
  import OverviewDescription from "./OverviewDescription.svelte";
  import OverviewLinks from "./OverviewLinks.svelte";
  import OverviewFindings from "./OverviewFindings.svelte";
  import OverviewTour from "./OverviewTour.svelte";
  import OverviewActivity from "./OverviewActivity.svelte";
  import EmptyBucket from "./EmptyBucket.svelte";
  import OverviewAside from "./OverviewAside.svelte";
  import { emptyBucketCopy } from "./emptyBucket";
  import { overviewHeader } from "./overviewModel";
  import { overviewSectionId } from "./overviewNav";

  interface Props {
    /** Defaults to the live snapshot; stories pass one. */
    snapshot?: AppSnapshot | null;
  }

  const { snapshot: snapshotOverride }: Props = $props();
  const snap = $derived(snapshotOverride !== undefined ? snapshotOverride : app.snapshot);

  const header = $derived(overviewHeader(snap));
  const github = $derived(snap ? githubStatusForActiveTab(snap) : null);
  const ai = $derived(snap?.ai ?? null);
  // OverviewLinks draws nothing without links; asking first keeps its block's
  // spacing from leaving a gap.
  const hasLinks = $derived(
    !!github &&
      extractPrLinks({
        body: github.body ?? "",
        comments: github.recent_comments,
        owner: github.owner,
        repo: github.repo,
        number: github.number,
      }).length > 0,
  );
  // Triage, risks, findings and tour belong to the view that made them
  // (ADR 0004); with this view's bucket empty and the other's not, one card
  // says where they are instead of four empty sections.
  const bucketEmpty = $derived(emptyBucketCopy(snap) !== null);
  // AgentOutputCard reads the live snapshot and draws nothing while idle; the
  // same test here keeps its wrapper's margin from leaving a gap.
  const hasAgentActivity = $derived(
    (app.snapshot?.agent_commands?.length ?? 0) > 0 || (app.snapshot?.agent_log?.length ?? 0) > 0,
  );

  const authorStyle = $derived.by(() => {
    if (!header.author) return "";
    const color = `var(${avatarToken(header.author)})`;
    return `color: ${color}; background: color-mix(in srgb, ${color} 18%, transparent)`;
  });

  const label = "m-0 mb-2.5 text-[11px] font-medium uppercase tracking-[.06em] text-fg-3";
</script>

<section class="flex-1 min-w-0 min-h-0 overflow-auto bg-bg" aria-label="Overview" data-testid="overview-view">
  <div
    class="mx-auto grid max-w-[1120px] grid-cols-[minmax(0,1fr)_272px] gap-12 px-7 pb-12 pt-7
      max-[880px]:grid-cols-[minmax(0,1fr)] max-[880px]:gap-8 max-[880px]:px-4 max-[880px]:pb-10 max-[880px]:pt-5"
  >
    <div class="min-w-0">
      <h1 class="m-0 mb-2 text-[24px] font-semibold leading-tight tracking-[-.015em] text-fg [text-wrap:balance]">
        {header.title}{#if header.prNumber != null}<span class="ml-2 font-normal text-fg-3">#{header.prNumber}</span>{/if}
      </h1>
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-[12px] text-fg-2">
        {#if header.author}
          <span class="inline-flex items-center gap-1.5">
            <span
              class="inline-grid h-[18px] w-[18px] place-items-center rounded-full text-[9px] font-semibold"
              style={authorStyle}
              aria-hidden="true"
            >{initials(header.author)}</span>{header.author}
          </span>
        {/if}
        {#if header.head}
          <span class="mono text-[11.5px]">{header.base ? `${header.base} ← ` : ""}{header.head}</span>
        {/if}
        {#if header.commits != null}
          <span>{header.commits} commit{header.commits === 1 ? "" : "s"}</span>
        {/if}
        {#if header.openedAt}
          <span>opened {longAge(header.openedAt)}</span>
        {/if}
      </div>

      <QuickActions snapshot={snap} />

      {#if github}
        <div class="mt-8">
          <OverviewDescription body={github.body} owner={github.owner} repo={github.repo} />
        </div>
        {#if hasLinks}
        <div class="mt-8">
          <OverviewLinks
            body={github.body}
            comments={github.recent_comments}
            owner={github.owner}
            repo={github.repo}
            number={github.number}
          />
        </div>
        {/if}
      {/if}

      {#if bucketEmpty}
        <div class="mt-8">
          <EmptyBucket snapshot={snap} />
        </div>
      {:else}
        <div id={overviewSectionId("triage")} class="mt-8 grid scroll-mt-4 gap-3">
          {#if ai?.triage}
            <TriageCard triage={ai.triage} />
          {:else}
            <div>
              <p class={label}>Triage</p>
              <p class="m-0 rounded-[10px] border border-line bg-surface px-4 py-3.5 text-[12.5px] text-fg-3">
                No triage for this view yet. Triage is a fast first read that says how to review the change.
              </p>
            </div>
          {/if}
          <!-- The TUI shows the checklist whenever the AI panel is up, with a
               fallback when the bucket has none; same gate here. -->
          {#if ai && (ai.checklist || ai.has_review_json)}
            <ChecklistCard checklist={ai.checklist} />
          {/if}
        </div>

        {#if ai && ai.file_risks.length > 0}
          <div class="mt-8">
            <FileRisksCard risks={ai.file_risks} />
          </div>
        {/if}

        <div class="mt-8">
          <OverviewFindings {ai} />
        </div>

        <div class="mt-8">
          <OverviewTour tour={snap?.tour ?? null} />
        </div>
      {/if}

      <!-- The active tab's agent commands and log, which the old Triage tab
           carried. Outside the bucket gate: a run in flight belongs to the
           tab, not to one view's artifacts. -->
      {#if hasAgentActivity}
        <div class="mt-8">
          <AgentOutputCard />
        </div>
      {/if}

      {#if ai}
        <div id={overviewSectionId("diagrams")} class="mt-8 scroll-mt-4">
          <p class={label}>Diagrams</p>
          <DiagramsCard {ai} />
        </div>
      {/if}

      {#if github}
        <div class="mt-8">
          <OverviewActivity {github} {ai} />
        </div>
      {/if}
    </div>

    <OverviewAside snapshot={snapshotOverride} />
  </div>
</section>
