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
 * Cmd/Ctrl+click additionally opens the usages popover (anchored at the click
 * point), listing every match for jump-to navigation. While the search bar is
 * open, clicking an identifier in the diff updates the search query instead
 * of toggling/clearing — the bar and the highlight never go out of sync.
 * Esc precedence (see `keyboard.ts`): popover → search bar → selection →
 * highlight.
 */

import { smartCaseSensitive, type MatchOptions } from "$lib/referenceHighlight";

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
    if (this.searchOpen) {
      if (ident !== null) this.setQuery(ident);
      return;
    }
    this.identifier = ident !== null && ident !== this.identifier ? ident : null;
    this.mode = "identifier";
    this.searchActiveIdx = -1;
    this.popoverOpen = false;
    this.popoverAnchor = null;
  }

  /**
   * Cmd/Ctrl+click semantics: highlight the identifier (even when it is
   * already the active one) and open the usages popover at the click point.
   * A Cmd/Ctrl+click that does not resolve to an identifier clears
   * everything, mirroring plain-click behavior. While the search bar is
   * open, routes to `setQuery` like a plain click (no popover).
   */
  openUsages(ident: string | null, anchor: { x: number; y: number }): void {
    if (this.searchOpen) {
      if (ident !== null) this.setQuery(ident);
      return;
    }
    if (ident === null) {
      this.clear();
      return;
    }
    this.identifier = ident;
    this.mode = "identifier";
    this.searchActiveIdx = -1;
    this.popoverAnchor = anchor;
    this.popoverOpen = true;
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
   * Open the Cmd+F search bar. An active identifier highlight is kept as the
   * prefilled query (switching it to substring/smart-case semantics).
   */
  openSearch(): void {
    this.searchOpen = true;
    this.popoverOpen = false;
    this.popoverAnchor = null;
    if (this.identifier !== null) {
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
