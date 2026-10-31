<script lang="ts">
  import type { AiSnapshot, GithubStatusSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { githubStatusForActiveTab } from "$lib/prUrl";
  import { longAge } from "$lib/relativeTime";
  import { openExternalUrl } from "$lib/openExternalUrl";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import type { GithubRef } from "$lib/prLinks";
  import ActivityBody from "./ActivityBody.svelte";
  import {
    avatarToken,
    buildActivity,
    initials,
    openedText,
    REVIEW_VERB,
    type ReviewState,
  } from "$lib/activityTimeline";

  interface Props {
    /** Defaults to the active tab's live GitHub status; stories pass one. */
    github?: GithubStatusSnapshot | null;
    ai?: AiSnapshot | null;
    /** Stories stand in for the backend's attachment fetch and tab opening. */
    loadImage?: (url: string) => Promise<string>;
    onOpenRef?: (ref: GithubRef, url: string, e: MouseEvent) => void;
  }

  const { github: githubOverride, ai: aiOverride, loadImage, onOpenRef }: Props = $props();
  const github = $derived(githubOverride !== undefined ? githubOverride : githubStatusForActiveTab(app.snapshot));
  const ai = $derived(aiOverride !== undefined ? aiOverride : (app.snapshot?.ai ?? null));
  const timeline = $derived(buildActivity(github, ai));

  const REVIEW_TEXT: Record<ReviewState, string> = {
    approved: "text-add-fg",
    changes: "text-del-fg",
    commented: "text-fg-3",
    dismissed: "text-fg-3",
  };
  const REVIEW_ICON: Record<ReviewState, string> = {
    approved: "✓",
    changes: "±",
    commented: "○",
    dismissed: "○",
  };

  const avatarStyle = (name: string) => {
    const color = `var(${avatarToken(name)})`;
    return `color: ${color}; background: color-mix(in srgb, ${color} 18%, transparent)`;
  };
  const ago = (iso: string | null) => (iso ? longAge(iso) : "");

  let draft = $state("");
  let posting = $state(false);

  // Posts to the PR straight away, like GitHub's own box: this is the
  // conversation, not a line comment that could wait in a pending review.
  async function comment(e: SubmitEvent) {
    e.preventDefault();
    const body = draft.trim();
    if (!body || posting) return;
    posting = true;
    try {
      await app.cmd("post_github_pr_comment", { body }, { throwOnError: true });
      draft = "";
    } catch {
      // app.cmd already reported it; the draft stays so nothing typed is lost.
    } finally {
      posting = false;
    }
  }
</script>

{#snippet avatar(name: string)}
  <span
    class="inline-grid h-[18px] w-[18px] shrink-0 place-items-center rounded-full text-[9px] font-semibold"
    style={avatarStyle(name)}
    aria-hidden="true"
  >{initials(name)}</span>
{/snippet}

{#snippet line(icon: string, iconClass: string, text: string, at: string | null)}
  <div class="grid grid-cols-[18px_minmax(0,1fr)] items-start gap-2.5 py-1.5 text-[12px] text-fg-2">
    <span class="grid h-[18px] place-items-center {iconClass}" aria-hidden="true">{icon}</span>
    <span class="min-w-0 [overflow-wrap:anywhere]">{text}{#if at}<span class="ml-1 text-fg-3">· {ago(at)}</span>{/if}</span>
  </div>
{/snippet}

<section class="min-w-0" aria-label="Activity" data-testid="overview-activity">
  <h2 class="m-0 mb-2.5 text-[11px] font-medium uppercase tracking-[.06em] text-fg-3">Activity</h2>

  <!-- One flexible track: a grid item's default min-width is its content, so
       a bot's wide table would otherwise widen the whole Overview column. -->
  <div class="grid grid-cols-[minmax(0,1fr)]">
    {#each timeline.events as event (event.key)}
      {#if event.kind === "opened"}
        {@render line("○", "text-fg-3", openedText(event.actor, event.commits), event.at)}
      {:else if event.kind === "requested"}
        {@render line("○", "text-fg-3", `Review requested from ${event.reviewer}${event.isTeam ? " (team)" : ""}`, null)}
      {:else if event.kind === "review" && !event.body}
        {@render line(REVIEW_ICON[event.state], REVIEW_TEXT[event.state], `${event.actor} ${REVIEW_VERB[event.state]}`, event.at)}
      {:else if event.kind === "review" || event.kind === "comment"}
        <div class="my-2 min-w-0 rounded-[10px] border border-line bg-surface px-3.5 py-3" data-testid="activity-comment">
          <div class="mb-1.5 flex min-w-0 items-center gap-2 text-[12px]">
            {@render avatar(event.actor)}
            <b class="min-w-0 truncate font-semibold text-fg">{event.actor}</b>
            {#if event.kind === "review"}
              <span class={REVIEW_TEXT[event.state]}>{REVIEW_VERB[event.state]}</span>
            {/if}
            <span class="shrink-0 text-fg-3">{ago(event.at)}</span>
          </div>
          <ActivityBody body={event.body} owner={github?.owner} repo={github?.repo} {loadImage} {onOpenRef} />
        </div>
      {:else if event.kind === "ai-review"}
        <!-- The review is the bucket's, not a reviewer's: no person's name or
             avatar here (CONTEXT.md, Lens). -->
        <div class="my-2 min-w-0 rounded-[10px] border border-line bg-surface px-3.5 py-3 [overflow-wrap:anywhere] {event.fresh ? '' : 'opacity-[.72]'}" data-testid="activity-ai-review">
          <div class="mb-1.5 flex items-center gap-2 text-[12px]">
            <span class="inline-grid h-[18px] w-[18px] shrink-0 place-items-center rounded-full bg-ai/20 text-[10px] text-ai" aria-hidden="true">✦</span>
            <b class="font-semibold text-fg">AI review</b>
            {#if event.at}<span class="text-fg-3">{ago(event.at)}</span>{/if}
            {#if !event.fresh}<span class="text-warning">stale</span>{/if}
            <span
              class="ml-auto inline-flex h-5 items-center rounded-[5px] px-[7px] text-[11px] font-medium
                {event.blocking > 0 ? 'bg-risk-high/14 text-risk-high' : 'bg-add-fg/13 text-add-fg'}"
              title="Open high-severity findings"
            >{event.blocking} blocking</span>
          </div>
          {#if event.summary}
            <MarkdownText text={event.summary} className="text-[13px] leading-[1.55] text-fg" />
          {:else}
            <p class="m-0 text-[13px] text-fg-2">{event.findings} finding{event.findings === 1 ? "" : "s"}, no summary.</p>
          {/if}
        </div>
      {/if}
    {:else}
      <p class="m-0 py-1.5 text-[12px] text-fg-3">
        {github ? "No activity on this PR yet." : "No pull request for this branch yet."}
      </p>
    {/each}
    {#if timeline.olderComments > 0 && github}
      <button
        type="button"
        class="justify-self-start py-1.5 text-[12px] text-fg-3 underline decoration-dotted underline-offset-[3px] hover:text-fg"
        onclick={() => github && void openExternalUrl(github.url)}
      >{timeline.olderComments} more comment{timeline.olderComments === 1 ? "" : "s"} on GitHub</button>
    {/if}
  </div>

  {#if github}
    <form
      class="mt-3 flex items-center gap-2 rounded-[10px] border border-border bg-surface py-2 pr-2.5 pl-2.5"
      onsubmit={comment}
    >
      <span
        class="inline-grid h-[18px] w-[18px] shrink-0 place-items-center rounded-full bg-accent text-[9px] font-semibold text-on-accent"
        aria-hidden="true"
        title="You"
      ><svg width="10" height="10" viewBox="0 0 16 16" fill="currentColor"><circle cx="8" cy="5" r="3" /><path d="M2.5 14a5.5 5.5 0 0 1 11 0z" /></svg></span>
      <input
        bind:value={draft}
        placeholder="Leave a comment on the PR…"
        aria-label="Comment on the pull request"
        class="min-w-0 flex-1 border-0 bg-transparent text-[13px] text-fg outline-none placeholder:text-fg-3"
      />
      <button
        type="submit"
        class="inline-flex h-7 items-center rounded-[7px] px-2.5 text-[12.5px] font-medium text-fg-2 hover:bg-hover hover:text-fg disabled:opacity-50"
        disabled={posting || !draft.trim()}
      >{posting ? "Posting…" : "Comment"}</button>
    </form>
  {/if}
</section>
