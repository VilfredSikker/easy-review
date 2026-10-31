<script lang="ts">
  import type { GithubRef } from "$lib/prLinks";
  import GithubMarkdown from "./GithubMarkdown.svelte";

  interface Props {
    body: string;
    owner?: string;
    repo?: string;
    loadImage?: (url: string) => Promise<string>;
    onOpenRef?: (ref: GithubRef, url: string, e: MouseEvent) => void;
  }

  const { body, owner, repo, loadImage, onOpenRef }: Props = $props();

  let clip = $state<HTMLDivElement | null>(null);
  let overflows = $state(false);
  let expanded = $state(false);

  // Measured rather than guessed from the text: a deploy bot's one-line table
  // and a reviewer's ten paragraphs are the same length in characters. The
  // observer also catches images that load after the first paint.
  $effect(() => {
    const el = clip;
    if (!el || expanded) return;
    const measure = () => (overflows = el.scrollHeight > el.clientHeight + 1);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    if (el.firstElementChild) ro.observe(el.firstElementChild);
    return () => ro.disconnect();
  });
</script>

<div class="activity-body" class:clipped={!expanded} class:faded={!expanded && overflows} bind:this={clip}>
  <GithubMarkdown {body} {owner} {repo} {loadImage} {onOpenRef} compact />
</div>
{#if overflows || expanded}
  <button
    type="button"
    class="mt-1.5 text-[12px] text-fg-3 underline decoration-dotted underline-offset-[3px] hover:text-fg"
    aria-expanded={expanded}
    onclick={() => (expanded = !expanded)}
  >{expanded ? "Show less" : "Show more"}</button>
{/if}

<style>
  .activity-body { min-width: 0; }
  /* About twelve lines of comment text: enough to read a human reply whole,
     short enough that a bot's deploy table does not push the composer away. */
  .activity-body.clipped { max-height: 240px; overflow: hidden; }
  .activity-body.faded { mask-image: linear-gradient(to bottom, black 70%, transparent); }
</style>
