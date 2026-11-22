/**
 * Selection moves over a tab's files and hunks, ported from the engine's
 * `crates/er-engine/src/navigation.rs` (P47.1b). The desktop holds its own
 * selection (ADR 0068), so a key press moves it here and makes no IPC call;
 * `navigation.test.ts` holds both to one fixture, so they cannot drift.
 */

/** What a file move selects; the indices are diff or watched file indices. */
export type FileMove =
  | { kind: "stay" }
  | { kind: "leave_watched" }
  | { kind: "file"; idx: number }
  | { kind: "watched"; idx: number }
  | { kind: "snap"; idx: number };

const file = (idx: number): FileMove => ({ kind: "file", idx });
const watchedFile = (idx: number): FileMove => ({ kind: "watched", idx });
const STAY: FileMove = { kind: "stay" };

/**
 * The next file down: through the visible diff files, on into the watched
 * files, and back round to the first diff file. A selection the filter hides
 * snaps to the first visible file.
 */
export function nextFile(
  selectedFile: number,
  selectedWatched: number | null,
  visible: readonly number[],
  watched: readonly number[],
): FileMove {
  if (selectedWatched !== null) {
    const pos = watched.indexOf(selectedWatched);
    if (pos === -1) return STAY;
    if (pos + 1 < watched.length) return watchedFile(watched[pos + 1]);
    return visible.length > 0 ? file(visible[0]) : { kind: "leave_watched" };
  }
  if (visible.length === 0) return watched.length > 0 ? watchedFile(watched[0]) : STAY;
  const pos = visible.indexOf(selectedFile);
  if (pos === -1) return { kind: "snap", idx: visible[0] };
  if (pos + 1 < visible.length) return file(visible[pos + 1]);
  return watched.length > 0 ? watchedFile(watched[0]) : file(visible[0]);
}

/** The mirror of {@link nextFile}: from the first diff file it wraps to the last watched file, or to the last diff file. */
export function prevFile(
  selectedFile: number,
  selectedWatched: number | null,
  visible: readonly number[],
  watched: readonly number[],
): FileMove {
  if (selectedWatched !== null) {
    const pos = watched.indexOf(selectedWatched);
    if (pos === -1) return STAY;
    if (pos > 0) return watchedFile(watched[pos - 1]);
    return visible.length > 0 ? file(visible[visible.length - 1]) : { kind: "leave_watched" };
  }
  if (visible.length === 0) return STAY;
  const pos = visible.indexOf(selectedFile);
  if (pos === -1) return { kind: "snap", idx: visible[0] };
  if (pos > 0) return file(visible[pos - 1]);
  return watched.length > 0 ? watchedFile(watched[watched.length - 1]) : file(visible[visible.length - 1]);
}

/** The hunk after `current`, if the file has one. */
export function nextHunk(current: number, hunkCount: number): number | null {
  return current + 1 < hunkCount ? current + 1 : null;
}

/** The hunk before `current`, if there is one. */
export function prevHunk(current: number): number | null {
  return current > 0 ? current - 1 : null;
}

/**
 * The first unreviewed file after the selected one, wrapping, with the
 * selected file itself last; null when every visible file is reviewed.
 * `visible` pairs each visible file's index with whether it is reviewed. A
 * selection the filter hides counts as the first file.
 */
export function jumpToUnreviewed(selectedFile: number, visible: readonly (readonly [number, boolean])[]): number | null {
  const pos = Math.max(0, visible.findIndex(([idx]) => idx === selectedFile));
  for (let offset = 1; offset <= visible.length; offset += 1) {
    const [idx, reviewed] = visible[(pos + offset) % visible.length];
    if (!reviewed) return idx;
  }
  return null;
}
