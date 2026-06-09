export type ReviewScope = "branch" | "unstaged" | "staged";

/** Maps snapshot diff mode to the scope string passed to Tauri AI commands. */
export function reviewScopeFromMode(mode: string | undefined): ReviewScope | null {
  if (mode === "pr" || mode === "branch") return "branch";
  if (mode === "unstaged" || mode === "staged") return mode;
  return null;
}

export function scopeDescriptionFromMode(mode: string | undefined): string {
  if (mode === "pr") return "PR diff vs base";
  if (mode === "branch") return "All changes vs base";
  if (mode === "unstaged") return "Working tree changes";
  if (mode === "staged") return "Staged changes only";
  return "Switch to All changes, PR Diff, Unstaged, or Staged";
}
