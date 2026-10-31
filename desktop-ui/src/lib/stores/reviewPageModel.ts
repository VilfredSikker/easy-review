// The review page (Overview / Tour / Diff) is shell state, not a diff mode:
// Overview is a frontend-only page, and Tour and Diff are the engine's own
// `DiffMode::Tour` and the mode it returns to. Keeping Overview out of the
// engine's mode set is what ADR 0006 asks for, so this module translates a page
// into a main view plus, at most, one `set_mode`.

import type { AppSnapshot, TabSummary } from "../types";
import type { MainViewMode } from "./app.svelte";

export type ReviewPage = "overview" | "tour" | "diff";

export interface SetModeArgs {
  mode: "tour" | "branch" | "pr_diff";
  prNumber?: number | null;
}

type ModeSnapshot = Pick<
  AppSnapshot,
  "mode" | "tour" | "features" | "detected_pr_number" | "github" | "pr"
>;

/** The main views that show a review page; every other main view replaces it. */
export function isReviewMainView(view: MainViewMode): boolean {
  return view === "overview" || view === "diff";
}

/** The page on screen, or null while a non-review main view (Settings, Export…) is up. */
export function currentReviewPage(
  view: MainViewMode,
  mode: AppSnapshot["mode"] | undefined,
): ReviewPage | null {
  if (view === "overview") return "overview";
  if (view !== "diff") return null;
  return mode === "tour" ? "tour" : "diff";
}

/**
 * Tour is off when the view is disabled in settings; `set_mode tour` would only
 * error. Overview is off until a component that renders it has registered: the
 * shell sends any main view it does not know to the Export page, so switching
 * to an unrendered Overview would open the wrong page and drop the right rail.
 */
export function reviewPageAvailable(
  page: ReviewPage,
  snap: ModeSnapshot | null,
  overviewRendered = false,
): boolean {
  if (page === "overview") return overviewRendered;
  if (page !== "tour") return true;
  return snap?.features?.viewTour ?? true;
}

/**
 * The `set_mode` that reaches `page`, or null when the engine's mode already
 * fits it. Leaving Tour returns to the diff the tour was generated from
 * (`tour_is_pr`, ADR 0025), not to whatever mode preceded it, so a PR tour
 * never drops the reader into the local branch diff.
 */
export function setModeArgsFor(page: ReviewPage, snap: ModeSnapshot): SetModeArgs | null {
  if (page === "tour") return snap.mode === "tour" ? null : { mode: "tour" };
  if (page === "overview" || snap.mode !== "tour") return null;
  if (snap.tour?.scope === "pr") {
    const prNumber =
      snap.detected_pr_number ?? snap.github?.number ?? snap.pr?.number ?? null;
    return { mode: "pr_diff", prNumber };
  }
  return { mode: "branch" };
}

/**
 * Identifies a tab across reorders and closes of its neighbours, which shift
 * `idx`. Two tabs on the same target share a key; they would also share a
 * review, so sharing the remembered page is harmless.
 */
export function reviewPageTabKey(snap: Pick<AppSnapshot, "tabs" | "active_tab"> | null): string | null {
  if (!snap) return null;
  const tab: TabSummary | undefined =
    snap.tabs.find((t) => t.is_active) ?? snap.tabs[snap.active_tab];
  if (!tab) return null;
  return `${tab.repo_root}|${tab.kind}|${tab.pr_number ?? tab.branch ?? ""}`;
}

/**
 * Remembers, per tab, the page the reader left it on. Tour versus Diff needs no
 * memory here: the engine keeps each tab's mode, so a tab left on its Tour comes
 * back in `DiffMode::Tour` by itself. What is remembered is Overview or not, and
 * whether the tab has been visited at all: a tab seen for the first time opens
 * on the Overview, since a fresh review starts from the summary, not the diff.
 */
export class ReviewPageMemory {
  private onOverview = new Set<string>();
  private visited = new Set<string>();
  private activeKey: string | null = null;

  /**
   * Note that `key` is now the active tab. Returns the main view to switch to,
   * or null to leave the current one. A non-review main view (Settings,
   * Export…) is left alone: the reader opened it on purpose, and switching
   * tabs under it should not close it. `overviewAvailable` is false until the
   * Overview can be drawn; a tab first seen then stays unvisited, so it still
   * gets the Overview default once it can.
   */
  follow(key: string | null, view: MainViewMode, overviewAvailable = true): MainViewMode | null {
    if (key === this.activeKey) return null;
    const leaving = this.activeKey;
    this.activeKey = key;
    if (!isReviewMainView(view)) return null;
    if (leaving !== null) {
      this.visited.add(leaving);
      if (view === "overview") this.onOverview.add(leaving);
      else this.onOverview.delete(leaving);
    }
    if (key === null) return null;
    if (!this.visited.has(key) && overviewAvailable) {
      this.visited.add(key);
      this.onOverview.add(key);
    }
    const next: MainViewMode = this.onOverview.has(key) ? "overview" : "diff";
    return next === view ? null : next;
  }
}
