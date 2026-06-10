/**
 * Reference-highlight state for the diff view (issue #69).
 *
 * Holds the identifier the user clicked in the diff. Every rendered diff line
 * checks its text against this identifier and marks word-boundary matches —
 * the diff view is viewport-virtualized, so only visible rows pay the cost.
 *
 * One global instance, like `diffSel`: the highlight spans every file in the
 * rendered diff, not just the clicked one.
 */

class ReferenceHighlight {
  identifier = $state<string | null>(null);

  /**
   * Click semantics: clicking a new identifier selects it; clicking the same
   * identifier again — or anywhere that does not resolve to an identifier
   * (`ident === null`) — clears the highlight.
   */
  toggle(ident: string | null): void {
    this.identifier = ident !== null && ident !== this.identifier ? ident : null;
  }

  clear(): void {
    this.identifier = null;
  }

  get active(): boolean {
    return this.identifier !== null;
  }
}

export const refHighlight = new ReferenceHighlight();
