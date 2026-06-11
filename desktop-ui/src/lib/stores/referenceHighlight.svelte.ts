/**
 * Reference-highlight state for the diff view (issues #69, #73).
 *
 * Holds the active highlight query. Every rendered diff line checks its text
 * against it and marks matches — the diff view is viewport-virtualized, so
 * only visible rows pay the cost. Two modes share the machinery:
 *
 * - `"identifier"` — the user clicked an identifier token in the diff.
 *   Whole-word, case-sensitive matching (unchanged from issue #69).
 * - `"query"` — the user typed into the Cmd+F search bar. Substring matching
 *   with smart-case (any uppercase letter in the query → case-sensitive).
 *
 * One global instance, like `diffSel`: the highlight spans every file in the
 * rendered diff, not just the clicked one.
 *
 * Cmd/Ctrl+click opens the usages popover (anchored at the click point),
 * listing every match for jump-to navigation — ALWAYS, whether or not the
 * search bar is open (the popover is the point of the gesture; an open bar
 * just gets its query synced to the clicked identifier). A plain click while
 * the search bar is open updates the search query instead of toggling — the
 * bar and the highlight never go out of sync. Routing lives in the pure
 * `identifierClickAction` helper (unit-tested in referenceHighlight.test.ts).
 * Esc precedence (see `keyboard.ts`): popover → search bar → selection →
 * highlight.
 */

import {
  identifierClickAction,
  smartCaseSensitive,
  type IdentifierClickAction,
  type MatchOptions,
} from "$lib/referenceHighlight";

export type HighlightMode = "identifier" | "query";

class ReferenceHighlight {
  identifier = $state<string | null>(null);
  mode = $state<HighlightMode>("identifier");
  popoverOpen = $state(false);
  popoverAnchor = $state<{ x: number; y: number } | null>(null);
  /** Cmd+F search bar visibility. */
  searchOpen = $state(false);
  /** Index into the flat match list while navigating; −1 = no current match. */
  searchActiveIdx = $state(-1);
  /**
   * Bumped on every `openSearch()` call. The search bar focuses + selects its
   * input in response, so Cmd+F focuses the field even when the bar is
   * already mounted (a plain `onMount` focus only runs once).
   */
  searchFocusEpoch = $state(0);

  /** Matching semantics for the active mode. */
  get matchOptions(): MatchOptions {
    if (this.mode === "query") {
      return {
        wordBoundary: false,
        caseSensitive: smartCaseSensitive(this.identifier ?? ""),
      };
    }
    return { wordBoundary: true, caseSensitive: true };
  }

  /**
   * Click semantics: clicking a new identifier selects it; clicking the same
   * identifier again — or anywhere that does not resolve to an identifier
   * (`ident === null`) — clears the highlight. A plain click never opens the
   * popover (and closes it if open). While the search bar is open, a click on
   * an identifier routes to `setQuery` instead and a non-identifier click is
   * a no-op (diff clicks never close or clear the bar).
   */
  toggle(ident: string | null): void {
    this.applyClick(identifierClickAction(ident, { cmd: false, searchOpen: this.searchOpen }), null);
  }

  /**
   * Cmd/Ctrl+click semantics: highlight the identifier (even when it is
   * already the active one) and open the usages popover at the click point —
   * ALWAYS, including while the search bar is open (the bar stays open and
   * its query syncs to the identifier). A Cmd/Ctrl+click that does not
   * resolve to an identifier clears everything, mirroring plain-click
   * behavior (no-op while the bar is open, so it is never cleared by a
   * stray click).
   */
  openUsages(ident: string | null, anchor: { x: number; y: number }): void {
    this.applyClick(identifierClickAction(ident, { cmd: true, searchOpen: this.searchOpen }), anchor);
  }

  private applyClick(
    action: IdentifierClickAction,
    anchor: { x: number; y: number } | null,
  ): void {
    switch (action.kind) {
      case "noop":
        return;
      case "clear":
        this.clear();
        return;
      case "set-query":
        this.setQuery(action.ident);
        return;
      case "toggle":
        this.identifier = action.ident !== this.identifier ? action.ident : null;
        this.mode = "identifier";
        this.searchActiveIdx = -1;
        this.popoverOpen = false;
        this.popoverAnchor = null;
        return;
      case "open-popover":
        this.identifier = action.ident;
        this.mode = "identifier";
        this.searchActiveIdx = -1;
        this.popoverAnchor = anchor;
        this.popoverOpen = true;
        return;
    }
  }

  /** Set the Cmd+F search query (empty string clears the highlight). */
  setQuery(q: string): void {
    this.identifier = q.length > 0 ? q : null;
    this.mode = "query";
    this.searchActiveIdx = -1;
    this.popoverOpen = false;
    this.popoverAnchor = null;
  }

  /**
   * Open the Cmd+F search bar. `prefill` (the diff's active text selection,
   * when there is one) takes priority as the initial query; otherwise an
   * active identifier highlight is kept as the prefilled query; with neither,
   * the bar opens empty. Always bumps `searchFocusEpoch` so the input is
   * focused and selected, even when the bar was already open.
   */
  openSearch(prefill: string | null = null): void {
    this.searchOpen = true;
    this.searchFocusEpoch += 1;
    this.popoverOpen = false;
    this.popoverAnchor = null;
    if (prefill !== null && prefill.length > 0) {
      this.identifier = prefill;
      this.mode = "query";
      this.searchActiveIdx = -1;
    } else if (this.identifier !== null) {
      this.mode = "query";
      this.searchActiveIdx = -1;
    }
  }

  /** Close the search bar AND clear the highlight (Esc does both). */
  closeSearch(): void {
    this.searchOpen = false;
    this.clear();
  }

  closePopover(): void {
    this.popoverOpen = false;
  }

  clear(): void {
    this.identifier = null;
    this.mode = "identifier";
    this.searchActiveIdx = -1;
    this.popoverOpen = false;
    this.popoverAnchor = null;
  }

  get active(): boolean {
    return this.identifier !== null;
  }
}

export const refHighlight = new ReferenceHighlight();
