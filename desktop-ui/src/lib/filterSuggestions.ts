import type { FilterSuggestionSnapshot } from "./types";

/** `kind:te`, `-kind:`, `+kind:code` — the draft is mid-way through one kind. */
const KIND_DRAFT = /^([+-]?)kind:([a-z]*)$/i;

/** Suggestions to list under the filter box for `draft`.
 *
 *  Empty draft: everything (kinds, presets, recent). While typing a `kind:`
 *  value: the kinds whose name starts with what is typed, keeping the `+`/`-`
 *  sign so `-kind:` offers excludes. Anything else: none, so the live file
 *  list below stays in view. */
export function visibleSuggestions(
  all: FilterSuggestionSnapshot[],
  draft: string,
): FilterSuggestionSnapshot[] {
  const trimmed = draft.trim();
  if (trimmed.length === 0) return all;
  const m = KIND_DRAFT.exec(trimmed);
  if (!m) return [];
  const [, sign, partial] = m;
  const typed = partial.toLowerCase();
  return all
    .filter((s) => s.kind === "kind" && s.name.startsWith(typed))
    .map((s) => ({ ...s, expr: `${sign}${s.expr}` }))
    .filter((s) => s.expr.toLowerCase() !== trimmed.toLowerCase());
}

/** Next highlighted row for an arrow key: -1 is "none", and moving past either
 *  end wraps, so Down from the input lands on the first row and Up on the last. */
export function moveHighlight(current: number, count: number, dir: 1 | -1): number {
  if (count === 0) return -1;
  if (current < 0) return dir === 1 ? 0 : count - 1;
  return (current + dir + count) % count;
}
