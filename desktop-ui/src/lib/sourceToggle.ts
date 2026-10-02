/** Which sides of the [Local Branch | PR Diff] toggle can be selected. The
 *  toggle is always shown so the current source stays visible; a side that
 *  cannot be reached is disabled with the reason as its tooltip. */
export interface SourceToggleState {
  localAvailable: boolean;
  prAvailable: boolean;
  localReason: string | null;
  prReason: string | null;
}

export function sourceToggleState(input: {
  prNumber: number | null | undefined;
  tabKind: string | undefined;
  localBranchCheckedOut: boolean | undefined;
}): SourceToggleState {
  const prAvailable = input.prNumber != null;
  const remote = input.tabKind === "remote_pr";
  // A branch tab with a PR but no checkout loads `gh pr diff` in Branch mode
  // too, so Local would only repeat PR Diff. Without a PR it diffs the branch
  // ref against its base, which needs no checkout.
  const prOnlyBranch =
    input.tabKind === "local_branch" && prAvailable && input.localBranchCheckedOut !== true;
  let localReason: string | null = null;
  if (remote) localReason = "Remote PR: no local checkout";
  else if (prOnlyBranch) localReason = "Branch is not checked out: same diff as PR Diff";
  return {
    localAvailable: localReason === null,
    prAvailable,
    localReason,
    prReason: prAvailable ? null : "No PR for this branch",
  };
}
