/** Tracks the last-seen change_token per tab idx for "new changes" indicators. */
let seen = $state<Record<number, string>>({});

/**
 * Record that the user has seen the current token for this tab.
 * Call when a tab becomes active.
 */
function markSeen(idx: number, token: string) {
  // No-op when unchanged so callers inside an $effect settle after one pass
  // instead of re-triggering on every run (new object ref → infinite loop).
  if (seen[idx] === token) return;
  seen = { ...seen, [idx]: token };
}

/**
 * Returns true if the tab has a non-empty token that differs from the last
 * seen token. Pure — never mutates state (mutating here would create a
 * read-write loop when called during render). A tab is only recorded via
 * `markSeen` (called for the active tab), so an unseen-but-never-activated
 * tab returns false: you haven't "looked" at it yet, so there's no baseline.
 */
function hasUnseen(idx: number, token: string): boolean {
  if (!token) return false;
  if (!(idx in seen)) return false;
  return seen[idx] !== token;
}

export const tabSeen = {
  markSeen,
  hasUnseen,
};
