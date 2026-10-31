<script lang="ts">
  import type { GithubRef } from "$lib/prLinks";
  import GithubMarkdown from "./GithubMarkdown.svelte";

  interface Props {
    /** The PR body as written on GitHub (markdown with raw HTML). */
    body: string;
    /** The PR's repo, so a bare `#12` links within it. */
    owner?: string;
    repo?: string;
    /** See GithubMarkdown; stories stand in for the backend fetch. */
    loadImage?: (url: string) => Promise<string>;
    onOpenRef?: (ref: GithubRef, url: string, e: MouseEvent) => void;
  }

  const { body, owner, repo, loadImage, onOpenRef }: Props = $props();
</script>

<p class="m-0 mb-2.5 text-[11px] font-medium uppercase tracking-[.06em] text-muted">Description</p>
{#if (body ?? "").trim()}
  <GithubMarkdown {body} {owner} {repo} {loadImage} {onOpenRef} />
{:else}
  <p class="m-0 text-[13px] italic text-muted">No description provided.</p>
{/if}
