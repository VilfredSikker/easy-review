<script lang="ts">
  import { hasColoredSyntaxSpans } from "$lib/highlightPlan";
  import { mergeWordDiffWithSyntax } from "$lib/mergeWordDiffWithSyntax";
  import { remapSpanColor } from "$lib/spanColorRemap";
  import type { SpanSnapshot } from "$lib/types";
  import type { Span as WordSpan } from "$lib/wordDiff";

  interface Props {
    text: string;
    wordSpans: WordSpan[] | null;
    syntaxSpans: SpanSnapshot[] | undefined;
    changedBgClass: string;
  }
  const { text, wordSpans, syntaxSpans, changedBgClass }: Props = $props();

  const coloredSyntaxSpans = $derived(
    hasColoredSyntaxSpans(syntaxSpans) ? syntaxSpans : undefined,
  );

  const segments = $derived(
    wordSpans ? mergeWordDiffWithSyntax(wordSpans, coloredSyntaxSpans) : null,
  );
</script>

{#if segments}
  {#each segments as seg, i (i)}
    {#if seg.changed}
      <span class={changedBgClass}>
        {#if seg.color}
          <span style="color: {remapSpanColor(seg.color)}">{seg.text}</span>
        {:else}
          {seg.text}
        {/if}
      </span>
    {:else if seg.color}
      <span style="color: {remapSpanColor(seg.color)}">{seg.text}</span>
    {:else}
      {seg.text}
    {/if}
  {/each}
{:else if coloredSyntaxSpans}
  {#each coloredSyntaxSpans as span, i (i)}
    {#if span.color}
      <span style="color: {remapSpanColor(span.color)}">{span.text}</span>
    {:else}
      {span.text}
    {/if}
  {/each}
{:else}
  {text}
{/if}
