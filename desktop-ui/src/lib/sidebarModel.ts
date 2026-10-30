import { contrastRatio } from "./diffContrast";
import { commitSeriesFor, type AppTheme } from "./themes";
import type { PrInfo } from "./types";

/** One letter for the project tile. Falls back to "?" so an unnamed repo
 *  still renders a tile rather than an empty square. */
export function projectMonogram(name: string): string {
  const ch = Array.from(name.trim())[0];
  return ch ? ch.toUpperCase() : "?";
}

/** Length of the categorical `--color-commit-N` series (ADR 0030). */
const SERIES_LEN = 6;

/** Stable slot in the categorical series per project name. A hash, not the
 *  list index: reordering or filtering projects must not recolour them. */
export function projectColorIndex(name: string): number {
  let h = 0;
  for (const ch of name) h = (h * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  return h % SERIES_LEN;
}

/** Tile colour from the categorical series, not the semantic status tokens:
 *  the tile sits above CI dots that use success/warning, and a project must
 *  not read as "passing" or "pending" by accident of its name. */
export function projectColorVar(name: string): string {
  return `var(--color-commit-${projectColorIndex(name) + 1})`;
}

/** Monogram colour for the tile. No single ink clears 4.5:1 on every series
 *  swatch in the light themes (white fails on amber, black on blue), so pick
 *  whichever of black or white reads better on this theme's swatch. */
export function projectTileInk(theme: AppTheme, name: string): string {
  const swatch = commitSeriesFor(theme)[projectColorIndex(name)];
  return contrastRatio("#000000", swatch) >= contrastRatio("#ffffff", swatch) ? "#000000" : "#ffffff";
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
