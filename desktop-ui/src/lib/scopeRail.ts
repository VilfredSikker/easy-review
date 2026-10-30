import type { AppSnapshot, CommitSummary } from "./types";

export type DiffModeName = AppSnapshot["mode"];
export type LocalScope = "branch" | "unstaged" | "staged";

export interface LocalScopeInputs {
  remote: boolean;
  /** A local-branch tab whose branch is not checked out has no working tree of
   *  its own, so Unstaged and Staged would describe some other checkout. */
  readOnly: boolean;
  features: { viewBranch: boolean; viewUnstaged: boolean; viewStaged: boolean };
}

/** The local scopes the segment offers. A remote PR tab has no working tree,
 *  so it gets none. */
export function localScopes({ remote, readOnly, features }: LocalScopeInputs): LocalScope[] {
  if (remote) return [];
  const scopes: LocalScope[] = [];
  if (features.viewBranch) scopes.push("branch");
  if (features.viewUnstaged && !readOnly) scopes.push("unstaged");
  if (features.viewStaged && !readOnly) scopes.push("staged");
  return scopes;
}

/** The segment button a mode lights. A commit picked from the branch's list,
 *  and the Tour, are still the local branch view. */
export function pressedScope(mode: DiffModeName): LocalScope | null {
  switch (mode) {
    case "unstaged":
    case "staged":
      return mode;
    case "branch":
    case "history":
    case "tour":
      return "branch";
    default:
      return null;
  }
}

/**
 * Whether the screen belongs to the PR diff view rather than the local branch
 * view. History has no source of its own: the engine returns it to the view it
 * was entered from but does not send which, so the caller passes the source it
 * last saw outside History. Mirrors `TabState::set_mode`'s `from_pr` rule.
 */
export function viewIsPr(
  mode: DiffModeName,
  tourScope: string | undefined,
  remote: boolean,
  lastSourceWasPr: boolean,
): boolean {
  if (remote || mode === "pr") return true;
  if (mode === "tour") return tourScope === "pr";
  if (mode === "history") return lastSourceWasPr;
  return false;
}

/** Footer label naming whose Reviewed count is shown. In History it names the
 *  commit, because those marks live in the History bucket (ADR 0004), not in
 *  the branch or PR bucket the commit belongs to. */
export function footerScopeLabel(
  mode: DiffModeName,
  isPr: boolean,
  selectedSha: string | null,
): string {
  switch (mode) {
    case "history":
      return selectedSha ? `Commit ${selectedSha.slice(0, 7)}` : "Commit";
    case "unstaged":
      return "Unstaged";
    case "staged":
      return "Staged";
    case "conflicts":
      return "Conflicts";
    case "hidden":
      return "Hidden";
    case "tour":
      return isPr ? "Tour · PR Diff" : "Tour · Local branch";
    default:
      return isPr ? "PR Diff" : "Local branch";
  }
}

/** A commit's +/-, or null when either side is unknown (a PR commit outside
 *  the local object DB). Unknown must not render as +0 −0. */
export function commitDiffstat(
  c: Pick<CommitSummary, "additions" | "deletions">,
): { additions: number; deletions: number } | null {
  if (c.additions == null || c.deletions == null) return null;
  return { additions: c.additions, deletions: c.deletions };
}
