<script lang="ts">
  import { hasColoredSyntaxSpans } from "$lib/highlightPlan";
  import { mergeWordDiffWithSyntax, type RenderSegment } from "$lib/mergeWordDiffWithSyntax";
  import { splitSegmentsByIdentifier, type RefSegment } from "$lib/referenceHighlight";
  import { refHighlight } from "$lib/stores/referenceHighlight.svelte";
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

  const baseSegments = $derived.by((): RenderSegment[] => {
    if (wordSpans) return mergeWordDiffWithSyntax(wordSpans, coloredSyntaxSpans);
    if (coloredSyntaxSpans) {
      return coloredSyntaxSpans.map((s) => ({
        text: s.text,
        color: s.color || undefined,
        changed: false,
      }));
    }
    return [{ text, changed: false }];
  });

  // Reference highlight (issue #69): when an identifier is selected, split
  // segments at word-boundary matches and mark them. No-op (same array back)
  // for lines without a match.
  const segments = $derived.by((): RefSegment[] => {
    const ident = refHighlight.identifier;
    return ident ? splitSegmentsByIdentifier(baseSegments, ident) : baseSegments;
  });
</script>

{#each segments as seg, i (i)}
  {#if seg.changed || seg.ref || seg.color}
    <span
      class="{seg.changed ? changedBgClass : ''}{seg.ref ? ' ref-highlight' : ''}"
      style={seg.color ? `color: ${remapSpanColor(seg.color)}` : undefined}
    >{seg.text}</span>
  {:else}{seg.text}{/if}
{/each}
