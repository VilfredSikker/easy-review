/** The filter the header's code pair applies: production files only, after the
 *  repo's `[file_kinds]` overrides — the same files the code count sums. */
export const CODE_FILTER = "kind:code";

/** Filter to apply when the code pair is clicked: on, or off when it is the
 *  active filter. `null` means clear. Any other active filter is replaced. */
export function nextCodeFilter(current: string | null | undefined): string | null {
  return current?.trim() === CODE_FILTER ? null : CODE_FILTER;
}
