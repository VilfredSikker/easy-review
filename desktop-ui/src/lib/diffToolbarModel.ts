import type { AppSnapshot, CommitSummary, FileSnapshot } from "./types";

type DiffModeName = AppSnapshot["mode"];

/**
 * The files the diff column draws. Hiding reviewed files is a presentation
 * filter over the snapshot, never the engine's unreviewed-only filter: that one
 * rebuilds `files` on every checkmark, which the reviewed-revision path exists
 * to avoid (ADR 0016). The Tour keeps its files, because its pillar layout is
 * built from the full list and a reviewed file there is already collapsed.
 */
export function visibleDiffFiles<F extends Pick<FileSnapshot, "reviewed">>(
  files: F[],
  hideReviewed: boolean,
  mode: DiffModeName,
): F[] {
  if (!hideReviewed || mode === "tour") return files;
  return files.filter((f) => !f.reviewed);
}

export type DiffEmptyState = "all-reviewed" | "nothing-staged" | "no-changes";

/** Why the diff column is empty, which decides what it tells the reader to do. */
export function diffEmptyState(mode: DiffModeName, totalFiles: number): DiffEmptyState {
  if (totalFiles > 0) return "all-reviewed";
  return mode === "staged" ? "nothing-staged" : "no-changes";
}

export const DIFF_EMPTY_TEXT: Record<DiffEmptyState, string> = {
  "all-reviewed": "All files reviewed. Turn off “Hide reviewed” to see them again.",
  "nothing-staged": "Nothing staged. Stage hunks from the Unstaged scope or with git add.",
  "no-changes": "No changes",
};

export interface CommitChip {
  sha: string;
  /** Position in the engine's commit list, the index the rail colours by;
   *  null when the commit is not in that list. */
  colorIndex: number | null;
}

export type CommitNote =
  | { kind: "commit"; chip: CommitChip; title: string }
  | { kind: "legend"; chips: CommitChip[] }
  | null;

const SHORT_SHA = 7;

/**
 * What the toolbar says about commits: the single commit on screen, or the
 * colour legend while lines are marked by commit. A single commit wins,
 * because marking lines within one commit would colour every line the same.
 */
export function commitNote(
  commits: readonly Pick<CommitSummary, "sha" | "title">[],
  selectedSha: string | null | undefined,
  markLines: boolean,
): CommitNote {
  if (selectedSha) {
    const i = commits.findIndex((c) => c.sha === selectedSha);
    return {
      kind: "commit",
      chip: { sha: selectedSha.slice(0, SHORT_SHA), colorIndex: i >= 0 ? i : null },
      title: i >= 0 ? commits[i].title : "",
    };
  }
  if (markLines && commits.length > 0) {
    return {
      kind: "legend",
      chips: commits.map((c, i) => ({ sha: c.sha.slice(0, SHORT_SHA), colorIndex: i })),
    };
  }
  return null;
}

export function reviewedCount(files: readonly Pick<FileSnapshot, "reviewed">[]): number {
  let n = 0;
  for (const f of files) if (f.reviewed) n++;
  return n;
}
