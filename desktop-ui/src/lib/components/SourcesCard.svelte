<script lang="ts">
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import { app } from "$lib/stores/app.svelte";
  import type { GithubStatusSnapshot } from "$lib/types";

  interface Props {
    /** Override for stories/tests; defaults to the live snapshot. */
    github?: GithubStatusSnapshot | null;
  }

  const { github: githubProp }: Props = $props();

  const github = $derived<GithubStatusSnapshot | null>(
    githubProp !== undefined ? githubProp : app.snapshot?.github ?? null
  );

  let expandChecks = $state(false);
  let manualRefreshing = $state(false);
  const refreshing = $derived(manualRefreshing || (app.snapshot?.bg_loading?.gh_status ?? false));

  const checkStats = $derived.by(() => {
    const g = github;
    if (!g) return { pass: 0, fail: 0, pending: 0, total: 0 };
    let pass = 0;
    let fail = 0;
    let pending = 0;
    for (const c of g.checks) {
      if (c.status === "PENDING") pending += 1;
      else if (c.conclusion === "SUCCESS" || c.conclusion === "pass") pass += 1;
      else if (c.conclusion === "FAILURE" || c.conclusion === "fail") fail += 1;
    }
    return { pass, fail, pending, total: g.checks.length };
  });

  function reviewLabel(decision: string | null): { text: string; tone: string } {
    if (decision === "APPROVED") return { text: "Approved", tone: "ok" };
    if (decision === "CHANGES_REQUESTED")
      return { text: "Changes requested", tone: "warn" };
    if (decision === "REVIEW_REQUIRED")
      return { text: "Review required", tone: "info" };
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
    if (github?.url) await app.cmd("open_url_in_browser", { url: github.url });
  }
</script>

<Card>
  <div class="mb-2 flex items-center justify-between">
    <SectionLabel>Sources</SectionLabel>
  </div>

  <div class="space-y-3 text-sm text-fg-2">
    <!-- GitHub row: live data when available, muted placeholder otherwise. -->
    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2 font-medium text-fg-1">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"
            ><path
              d="M12 0C5.4 0 0 5.4 0 12c0 5.3 3.4 9.8 8.2 11.4.6.1.8-.3.8-.6v-2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2 1-.3 2-.4 3-.4s2 .1 3 .4c2.3-1.6 3.3-1.2 3.3-1.2.7 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6C20.6 21.8 24 17.3 24 12c0-6.6-5.4-12-12-12z"
            /></svg
          >
          <span>GitHub</span>
          {#if github}
            <button
              type="button"
              class="text-accent hover:text-accent/80 underline underline-offset-2 font-mono text-xs"
              onclick={onOpenPr}
              title="Open PR #{github.number} in browser"
            >
              #{github.number}
            </button>
          {/if}
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
          ><path d="M21 12a9 9 0 1 1-3-6.7L21 8" /><path d="M21 3v5h-5" /></svg
          >
        </button>
      </div>

      {#if !github}
        <div class="text-xs text-fg-3 italic pl-5 flex items-center gap-2">
          {#if refreshing}
            <svg
              width="11"
              height="11"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              class="animate-spin"
            ><path d="M21 12a9 9 0 1 1-3-6.7L21 8" /><path d="M21 3v5h-5" /></svg>
            <span>Looking up GitHub PR…</span>
          {:else}
            <span>No GitHub data yet</span>
          {/if}
        </div>
      {:else}
        <div class="pl-5 space-y-2">
          <!-- Chip row: PR state + draft + review decision + mergeable -->
          <div class="flex flex-wrap items-center gap-1.5">
            {#each [stateLabel(github.state, github.is_draft)] as s}
              <span class="text-[10px] px-1.5 py-0.5 rounded border {toneClass(s.tone)}"
                >{s.text}</span
              >
            {/each}
            {#each [reviewLabel(github.review_decision)] as r}
              <span class="text-[10px] px-1.5 py-0.5 rounded border {toneClass(r.tone)}"
                >{r.text}</span
              >
            {/each}
            {#if mergeableLabel(github.mergeable)}
              {@const m = mergeableLabel(github.mergeable)!}
              <span class="text-[10px] px-1.5 py-0.5 rounded border {toneClass(m.tone)}"
                >{m.text}</span
              >
            {/if}
          </div>

          <!-- Checks summary -->
          {#if checkStats.total > 0}
            <button
              type="button"
              class="flex items-center gap-2 text-xs hover:text-fg-1"
              onclick={() => (expandChecks = !expandChecks)}
              data-testid="checks-toggle"
            >
              <span class:text-add-fg={checkStats.fail === 0 && checkStats.pending === 0}
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
                      <a
                        href={c.url}
                        target="_blank"
                        rel="noopener noreferrer"
                        class="text-fg-2 hover:text-fg-1 hover:underline truncate">{c.name}</a
                      >
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
                <span class="text-[10px] px-1.5 py-0.5 rounded border border-hairline text-fg-3"
                  >{label}</span
                >
              {/each}
            </div>
          {/if}

          <!-- Counts -->
          <div class="flex items-center gap-3 text-xs text-fg-3">
            <span>{github.comments_count} comment{github.comments_count === 1 ? "" : "s"}</span>
            <span>{github.reviews_count} review{github.reviews_count === 1 ? "" : "s"}</span>
          </div>
        </div>
      {/if}
    </div>

    <!-- Figma + Linear: placeholders -->
    <div class="flex items-center gap-2 text-fg-3">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
        ><rect x="3" y="3" width="18" height="18" rx="4" /></svg
      >
      <span>Figma</span>
      <span class="text-[10px] text-fg-3 italic">— not connected</span>
    </div>
    <div class="flex items-center gap-2 text-fg-3">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
        ><circle cx="12" cy="12" r="10" /></svg
      >
      <span>Linear</span>
      <span class="text-[10px] text-fg-3 italic">— not connected</span>
    </div>
  </div>
</Card>
