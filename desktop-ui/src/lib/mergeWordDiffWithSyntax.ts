import { hasColoredSyntaxSpans } from "./highlightPlan";
import type { SpanSnapshot } from "./types";
import type { Span as WordSpan } from "./wordDiff";

export interface RenderSegment {
  text: string;
  color?: string;
  changed: boolean;
}

function coalesceRenderSegments(segs: RenderSegment[]): RenderSegment[] {
  if (segs.length <= 1) return segs;
  const out: RenderSegment[] = [];
  for (const s of segs) {
    const last = out[out.length - 1];
    if (last && last.changed === s.changed && last.color === s.color) {
      last.text += s.text;
    } else {
      out.push({ ...s });
    }
  }
  return out;
}

/**
 * Intersect word-diff regions with Shiki syntax spans so intra-line change
 * backgrounds coexist with per-token colors on add/del lines.
 */
export function mergeWordDiffWithSyntax(
  wordSpans: WordSpan[],
  syntaxSpans: SpanSnapshot[] | undefined,
): RenderSegment[] {
  if (!hasColoredSyntaxSpans(syntaxSpans)) {
    return wordSpans.map((w) => ({ text: w.text, changed: w.changed }));
  }

  const coloredSpans = syntaxSpans ?? [];
  const result: RenderSegment[] = [];
  let synIdx = 0;
  let synOffset = 0;

  for (const w of wordSpans) {
    let remaining = w.text.length;
    let wOffset = 0;

    while (remaining > 0 && synIdx < coloredSpans.length) {
      const syn = coloredSpans[synIdx];
      const synRemaining = syn.text.length - synOffset;
      const take = Math.min(remaining, synRemaining);
      result.push({
        text: w.text.slice(wOffset, wOffset + take),
        color: syn.color || undefined,
        changed: w.changed,
      });
      wOffset += take;
      remaining -= take;
      synOffset += take;
      if (synOffset >= syn.text.length) {
        synIdx++;
        synOffset = 0;
      }
    }

    if (remaining > 0) {
      result.push({
        text: w.text.slice(wOffset),
        changed: w.changed,
      });
    }
  }

  return coalesceRenderSegments(result);
}
