/**
 * Reference-highlight state for the diff view (issue #69).
 *
 * Holds the identifier the user clicked in the diff. Every rendered diff line
 * checks its text against this identifier and marks word-boundary matches —
 * the diff view is viewport-virtualized, so only visible rows pay the cost.
 *
 * One global instance, like `diffSel`: the highlight spans every file in the
 * rendered diff, not just the clicked one.
 *
 * Cmd/Ctrl+click additionally opens the usages popover (anchored at the click
 * point), listing every match for jump-to navigation. Esc closes the popover
 * first; a second Esc clears the highlight (see `keyboard.ts`).
 */

class ReferenceHighlight {
  identifier = $state<string | null>(null);
  popoverOpen = $state(false);
  popoverAnchor = $state<{ x: number; y: number } | null>(null);

  /**
   * Click semantics: clicking a new identifier selects it; clicking the same
   * identifier again — or anywhere that does not resolve to an identifier
   * (`ident === null`) — clears the highlight. A plain click never opens the
   * popover (and closes it if open).
   */
  toggle(ident: string | null): void {
    this.identifier = ident !== null && ident !== this.identifier ? ident : null;
    this.popoverOpen = false;
    this.popoverAnchor = null;
  }

  /**
   * Cmd/Ctrl+click semantics: highlight the identifier (even when it is
   * already the active one) and open the usages popover at the click point.
   * A Cmd/Ctrl+click that does not resolve to an identifier clears
   * everything, mirroring plain-click behavior.
   */
  openUsages(ident: string | null, anchor: { x: number; y: number }): void {
    if (ident === null) {
      this.clear();
      return;
    }
    this.identifier = ident;
    this.popoverAnchor = anchor;
    this.popoverOpen = true;
  }

  closePopover(): void {
    this.popoverOpen = false;
  }

  clear(): void {
    this.identifier = null;
    this.popoverOpen = false;
    this.popoverAnchor = null;
  }

  get active(): boolean {
    return this.identifier !== null;
  }
}

export const refHighlight = new ReferenceHighlight();
