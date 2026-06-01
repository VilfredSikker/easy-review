import type { Vote } from "$lib/types/arena";

/** Background tint for matrix vote cells. */
export function voteCellClass(vote: Vote | undefined): string {
  if (!vote) return "";
  switch (vote) {
    case "keep":
      return "bg-[rgba(78,201,164,0.12)]";
    case "drop":
      return "bg-[rgba(120,130,150,0.08)]";
    case "escalate":
      return "bg-[rgba(255,107,107,0.12)]";
    case "merge":
      return "bg-[rgba(127,135,255,0.14)]";
    case "lower":
      return "bg-[rgba(255,196,87,0.1)]";
    case "flag":
      return "bg-[rgba(255,196,87,0.1)]";
    case "propose":
      return "bg-[rgba(255,122,43,0.1)]";
    default:
      return "";
  }
}
