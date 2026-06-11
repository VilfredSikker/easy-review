import type { Vote } from "$lib/types/arena";

/** Background tint for matrix vote cells (derived from the arena palette vars). */
export function voteCellClass(vote: Vote | undefined): string {
  if (!vote) return "";
  switch (vote) {
    case "keep":
      return "bg-[color-mix(in_srgb,var(--arena-ok)_12%,transparent)]";
    case "drop":
      return "bg-[color-mix(in_srgb,var(--arena-fg-subtle)_8%,transparent)]";
    case "escalate":
      return "bg-[color-mix(in_srgb,var(--arena-err)_12%,transparent)]";
    case "merge":
      return "bg-[color-mix(in_srgb,var(--arena-periwinkle)_14%,transparent)]";
    case "lower":
      return "bg-[color-mix(in_srgb,var(--arena-warn)_10%,transparent)]";
    case "flag":
      return "bg-[color-mix(in_srgb,var(--arena-warn)_10%,transparent)]";
    case "propose":
      return "bg-[color-mix(in_srgb,var(--arena-orange)_10%,transparent)]";
    default:
      return "";
  }
}
