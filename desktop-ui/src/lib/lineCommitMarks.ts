import { commitColorVar } from "./commitColors";
import { lineCommitSha, type LineCommits } from "./lineCommits";
import type { CommitSummary, LineSnapshot } from "./types";

/** Colour slot per sha: the position in the engine's commit list, the same
 *  index the commits list and the toolbar legend colour by. */
export function commitSlots(commits: readonly Pick<CommitSummary, "sha">[]): Map<string, number> {
  return new Map(commits.map((c, i) => [c.sha, i]));
}

/**
 * Mark lines is a Diff view aid over a commit range. A single commit on screen
 * turns it off, because every line would take that one colour.
 */
export function markLinesActive(
  markLines: boolean,
  selectedSha: string | null | undefined,
  tour: boolean,
): boolean {
  return markLines && !selectedSha && !tour;
}

/** The inset bar colour for one diff line, or null when it has none: context
 *  lines, lines that predate the range, and commits outside the listed ones. */
export function lineCommitColor(
  attribution: LineCommits | null,
  slots: ReadonlyMap<string, number>,
  line: Pick<LineSnapshot, "kind" | "old_num" | "new_num"> | null,
): string | null {
  if (!attribution || !line) return null;
  let sha: string | null = null;
  if (line.kind === "add" && line.new_num != null) sha = lineCommitSha(attribution, "add", line.new_num);
  else if (line.kind === "del" && line.old_num != null) sha = lineCommitSha(attribution, "del", line.old_num);
  if (sha === null) return null;
  const slot = slots.get(sha);
  return slot === undefined ? null : commitColorVar(slot);
}

export interface FileCommitChip {
  sha: string;
  title: string;
  color: string;
}

/** The file header's commit chip: the oldest listed commit that touched the
 *  file, as the prototype's `commitIdx` picks the first one. */
export function fileCommitChip(
  attribution: LineCommits | null,
  commits: readonly Pick<CommitSummary, "sha" | "title">[],
): FileCommitChip | null {
  if (!attribution) return null;
  const slots = commitSlots(commits);
  for (const sha of attribution.commits) {
    const slot = slots.get(sha);
    if (slot !== undefined) {
      return { sha: sha.slice(0, 7), title: commits[slot].title, color: commitColorVar(slot) };
    }
  }
  return null;
}

/** "unpushed" when a commit that touched the file has not reached a remote:
 *  the part of the diff a pull request would not show yet. */
export function fileLocalState(
  attribution: LineCommits | null,
  commits: readonly Pick<CommitSummary, "sha" | "unpushed">[],
): "unpushed" | null {
  if (!attribution) return null;
  const unpushed = new Set(commits.filter((c) => c.unpushed).map((c) => c.sha));
  if (unpushed.size === 0) return null;
  return attribution.commits.some((sha) => unpushed.has(sha)) ? "unpushed" : null;
}

/** Attribution runs one `git blame` per file, so it is asked for only while a
 *  reader can use it: lines are marked, or a commit is unpushed and the file
 *  header's local chip needs to know which files it touched. */
export function attributionWanted(
  markActive: boolean,
  commits: readonly Pick<CommitSummary, "unpushed">[],
  tour: boolean,
): boolean {
  if (tour) return false;
  return markActive || commits.some((c) => c.unpushed);
}
