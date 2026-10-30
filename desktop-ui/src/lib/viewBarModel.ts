import { annotationCounts, type AnnotationCounts } from "./annotationCounts";
import { commentThreadsFromDiff } from "./commentVisibility";
import { themeByName } from "./themes";
import type { AppSnapshot, GithubStatusSnapshot } from "./types";

/**
 * Each palette's counterpart on the other side of light/dark. Eight themes
 * do not split into tidy pairs, so the three light ones each name the dark
 * theme they were drawn from, and every dark theme lands on the light one
 * closest in character.
 */
const OPPOSITE_THEME: Record<string, string> = {
  graphite: "daylight",
  slate: "daylight",
  midnight: "daylight",
  ember: "paper",
  "contrast-dark": "contrast-light",
  daylight: "graphite",
  paper: "ember",
  "contrast-light": "contrast-dark",
};

/**
 * The theme the view bar's light/dark button switches to. `previous` is the
 * theme the button last switched away from: going back returns to it, so
 * slate → daylight → slate round-trips instead of landing on graphite.
 */
export function themeToggleTarget(current: string | null | undefined, previous: string | null): string {
  const now = themeByName(current);
  if (previous) {
    const prev = themeByName(previous);
    if (prev.light !== now.light) return prev.name;
  }
  return OPPOSITE_THEME[now.name] ?? (now.light ? "graphite" : "daylight");
}

export interface ApproveGate {
  /** A PR exists and is open, so there is something to approve. */
  visible: boolean;
  enabled: boolean;
  /** Why it is disabled, for the tooltip. */
  reason: string | null;
}

/**
 * Same rule as the Branch card's Approve: GitHub refuses an approval from the
 * PR's author, so the button is shown but disabled with the reason rather
 * than failing on click.
 */
export function approveGate(github: GithubStatusSnapshot | null | undefined, submitting: boolean): ApproveGate {
  if (!github || github.state !== "OPEN") return { visible: false, enabled: false, reason: null };
  if (github.is_authored_by_me) {
    return { visible: true, enabled: false, reason: "GitHub does not allow approving your own PR" };
  }
  return { visible: true, enabled: !submitting, reason: null };
}

type CountSnapshot = Pick<AppSnapshot, "ai" | "files">;

/**
 * Totals for the display menu rows. GitHub comments are read from the hunks
 * the diff draws, as the Comments panel does, so the menu never counts a
 * comment the diff would not show.
 */
export function displayMenuCounts(snapshot: CountSnapshot | null | undefined): AnnotationCounts {
  const ai = snapshot?.ai;
  if (!ai) return annotationCounts(null, null);
  const threads = [
    ...commentThreadsFromDiff(ai.threads, snapshot?.files),
    ...ai.threads.filter((t) => t.kind !== "comment"),
  ];
  return annotationCounts(threads, ai.findings, ai.resolved_findings);
}
