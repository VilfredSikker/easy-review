import type { FileSnapshot, PillarSnapshot } from "./types";

export const OTHER_FILES_TITLE = "Other files";

export interface PillarFileGroup {
  /** Pillar id; null for the trailing "Other files" group. */
  pillarId: string | null;
  title: string;
  files: FileSnapshot[];
}

/** Joins the diff's files to tour pillars by path. A tour covers the files the
 *  model chose to explain, so files it left out land in a trailing "Other
 *  files" group rather than disappearing from the list. A file claimed by two
 *  pillars is listed under the first so counts add up to the diff. */
export function filesByPillar(
  pillars: readonly PillarSnapshot[],
  files: readonly FileSnapshot[],
): PillarFileGroup[] {
  const byPath = new Map(files.map((f) => [f.path, f]));
  const claimed = new Set<string>();
  const groups: PillarFileGroup[] = [];

  for (const p of pillars) {
    const members: FileSnapshot[] = [];
    for (const ref of p.files) {
      const f = byPath.get(ref.path);
      if (f && !claimed.has(f.path)) {
        claimed.add(f.path);
        members.push(f);
      }
    }
    groups.push({ pillarId: p.id, title: p.title, files: members });
  }

  const rest = files.filter((f) => !claimed.has(f.path));
  if (rest.length > 0) {
    groups.push({ pillarId: null, title: OTHER_FILES_TITLE, files: rest });
  }
  return groups;
}
