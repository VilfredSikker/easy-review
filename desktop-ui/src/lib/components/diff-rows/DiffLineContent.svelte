<script lang="ts">
  import { correctSyntaxColor, type DiffBgKind } from "$lib/diffContrast";
  import { hasColoredSyntaxSpans } from "$lib/highlightPlan";
  import { mergeWordDiffWithSyntax, type RenderSegment } from "$lib/mergeWordDiffWithSyntax";
  import { splitSegmentsByIdentifier, type RefSegment } from "$lib/referenceHighlight";
  import { app } from "$lib/stores/app.svelte";
  import { refHighlight } from "$lib/stores/referenceHighlight.svelte";
  import { themeByName } from "$lib/themes";
  import type { SpanSnapshot } from "$lib/types";
  import type { Span as WordSpan } from "$lib/wordDiff";

  interface Props {
    text: string;
    wordSpans: WordSpan[] | null;
    syntaxSpans: SpanSnapshot[] | undefined;
    changedBgClass: string;
    /** Diff background this line renders on — drives syntax contrast correction. */
    kind: DiffBgKind;
  }
  const { text, wordSpans, syntaxSpans, changedBgClass, kind }: Props = $props();

  const theme = $derived(themeByName(app.snapshot?.theme));

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
  // segments at matches and mark them. The store's matchOptions select
  // word-boundary (identifier click) vs substring/smart-case (Cmd+F search)
  // semantics. No-op (same array back) for lines without a match.
  const segments = $derived.by((): RefSegment[] => {
    const ident = refHighlight.identifier;
    return ident
      ? splitSegmentsByIdentifier(baseSegments, ident, refHighlight.matchOptions)
      : baseSegments;
  });
</script>

{#each segments as seg, i (i)}
  {#if seg.changed || seg.ref || seg.color}
    <span
      class="{seg.changed ? changedBgClass : ''}{seg.ref ? ' ref-highlight' : ''}"
      style={seg.color ? `color: ${correctSyntaxColor(seg.color, theme, kind, seg.changed)}` : undefined}
    >{seg.text}</span>
  {:else}{seg.text}{/if}
{/each}
