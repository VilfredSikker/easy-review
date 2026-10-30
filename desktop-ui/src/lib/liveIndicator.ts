export type LiveState = "live" | "stale" | "local";

export interface LiveIndicator {
  state: LiveState;
  label: string;
}

export interface LiveInput {
  /** The view is a PR diff (remote PR tab, or the PR side of the Local|PR toggle). */
  isPrView: boolean;
  /** `snapshot.diff_stale`: the PR head moved past what the diff was built from. */
  diffStale: { message?: string } | null | undefined;
  branch: string;
}

/** Stale only applies to a PR diff: a local worktree is re-read from disk as
 *  it changes, so it cannot be behind anything. Staleness wins over live so a
 *  moved head is never presented as current. */
export function deriveLiveIndicator(input: LiveInput): LiveIndicator {
  if (input.isPrView && input.diffStale) {
    return {
      state: "stale",
      label: input.diffStale.message ? `Stale · ${input.diffStale.message}` : "Stale",
    };
  }
  if (!input.isPrView) return { state: "local", label: "Live · local worktree" };
  return { state: "live", label: input.branch ? `Live · ${input.branch}` : "Live" };
}
