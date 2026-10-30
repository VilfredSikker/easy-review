import { commitColorVar } from "./commitColors";
import type { PrInfo } from "./types";

/** One letter for the project tile. Falls back to "?" so an unnamed repo
 *  still renders a tile rather than an empty square. */
export function projectMonogram(name: string): string {
  const ch = Array.from(name.trim())[0];
  return ch ? ch.toUpperCase() : "?";
}

/** Stable tile colour per project name, drawn from the commit accent cycle
 *  so it follows the active theme (ADR 0030). A hash, not the list index:
 *  reordering or filtering projects must not recolour them. */
export function projectColorVar(name: string): string {
  let h = 0;
  for (const ch of name) h = (h * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  return commitColorVar(h);
}

export type PrCiState = "ok" | "fail" | "pend" | null;

export function prCiState(pr: Pick<PrInfo, "checks_state">): PrCiState {
  switch (pr.checks_state) {
    case "PASSING":
      return "ok";
    case "FAILING":
      return "fail";
    case "PENDING":
      return "pend";
    default:
      return null;
  }
}

/** The PR is waiting on the signed-in user: open, not a draft, and not yet
 *  approved by them, and it came from the review queue rather than their own. */
export function prNeedsMyReview(
  pr: Pick<PrInfo, "state" | "is_draft" | "approved_by_me">,
  inReviewQueue: boolean,
): boolean {
  return inReviewQueue && pr.state === "OPEN" && !pr.is_draft && !pr.approved_by_me;
}
