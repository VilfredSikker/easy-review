import { app } from "./app.svelte";
import {
  currentReviewPage,
  reviewLayers,
  reviewPageAvailable,
  setModeArgsFor,
  type ReviewLayers,
  type ReviewPage,
} from "./reviewPageModel";

export type { ReviewPage };

let overviewRendered = $state(false);
let overviewVisited = $state(false);

/**
 * Overview / Tour / Diff for the active tab. Derived from `app.mainView` and
 * the engine's mode rather than stored, so a mode change made anywhere else
 * (the palette, a Tour link) shows up here without a second source of truth.
 * The per-tab memory lives in `ReviewPageMemory`, driven by the app store.
 */
export const reviewPage = {
  get current(): ReviewPage | null {
    return currentReviewPage(app.mainView, app.snapshot?.mode);
  },

  /** Which review pages the shell keeps mounted, and which one is on screen. */
  get layers(): ReviewLayers {
    return reviewLayers(app.mainView, overviewVisited);
  },

  available(page: ReviewPage): boolean {
    return reviewPageAvailable(page, app.snapshot, overviewRendered);
  },

  /**
   * Called by the component that renders the `overview` main view. Until then
   * the Overview is unavailable, so `1` and `show("overview")` do nothing
   * rather than land on a main view the shell cannot draw.
   */
  registerOverview(): void {
    overviewRendered = true;
  },

  /** Called by the shell once the Overview has been on screen, so it stays mounted. */
  noteOverviewShown(): void {
    overviewVisited = true;
  },

  show(page: ReviewPage): void {
    const snap = app.snapshot;
    if (!reviewPageAvailable(page, snap, overviewRendered)) return;
    app.setMainView(page === "overview" ? "overview" : "diff");
    const args = snap ? setModeArgsFor(page, snap) : null;
    if (args) void app.cmd("set_mode", { ...args });
  },
};
