export type ReviewScope = "branch" | "unstaged" | "staged";

/** Maps snapshot diff mode to the scope string passed to Tauri AI commands. */
export function reviewScopeFromMode(mode: string | undefined): ReviewScope | null {
  // Guide (tour) is the branch diff regrouped into pillars — it reviews the
  // same change set as branch/PR, and shares the same branch review bucket.
  if (mode === "pr" || mode === "branch" || mode === "tour") return "branch";
  if (mode === "unstaged" || mode === "staged") return mode;
  return null;
}

export function scopeDescriptionFromMode(mode: string | undefined): string {
  if (mode === "pr") return "PR diff vs base";
  if (mode === "branch" || mode === "tour") return "All changes vs base";
  if (mode === "unstaged") return "Working tree changes";
  if (mode === "staged") return "Staged changes only";
  return "Switch to All changes, PR Diff, Unstaged, or Staged";
}
