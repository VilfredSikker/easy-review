import type { Confidence } from "$lib/types";

// View state for findings the reader has filtered out of the diff.
//
// Session-scoped on purpose. A remembered "show resolved" changes what a later
// diff shows before anyone has looked at it, and a remembered gate hides
// findings in a review it was never set for — the same reason the engine's
// layer toggles reset per tab (`.scratch/finding-gating/spec.md`).
class FindingsVisibilityStore {
  showResolved = $state(false);

  // A gate the reader set by hand. `null` follows the review: the snapshot
  // carries the default the engine resolved (`min_trust_default`).
  minTrustOverride = $state<Confidence | null>(null);

  toggleResolved() {
    this.showResolved = !this.showResolved;
  }

  setMinTrust(level: Confidence | null) {
    this.minTrustOverride = level;
  }

  // The gate in force for a review whose engine-resolved default is `fallback`.
  minTrust(fallback: Confidence): Confidence {
    return this.minTrustOverride ?? fallback;
  }
}

export const findingsVisibility = new FindingsVisibilityStore();
