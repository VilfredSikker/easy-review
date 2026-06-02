const ELLIPSIS = "…";

/**
 * Shorten a repo-relative file path to fit `maxChars`, keeping the filename visible.
 * Directory prefix is truncated from the start: `some/very/…/file.rs`.
 */
export function shortenPath(path: string, maxChars: number): string {
  if (maxChars <= 0) return ELLIPSIS;
  if (path.length <= maxChars) return path;

  const slash = path.lastIndexOf("/");
  const name = slash === -1 ? path : path.slice(slash + 1);
  const dir = slash === -1 ? "" : path.slice(0, slash + 1);

  if (name.length <= maxChars) {
    const remaining = maxChars - name.length - 4; // room for "…/"
    if (remaining > 0 && dir.length > 0) {
      const dirPart = dir.slice(0, -1).slice(0, remaining);
      return `${dirPart}${ELLIPSIS}/${name}`;
    }
    return name;
  }

  return `${name.slice(0, maxChars - 1)}${ELLIPSIS}`;
}

/** Split a (possibly shortened) path for breadcrumb rendering. */
export function splitPathForDisplay(path: string): { dir: string; name: string } {
  const slash = path.lastIndexOf("/");
  if (slash === -1) return { dir: "", name: path };
  return { dir: path.slice(0, slash + 1), name: path.slice(slash + 1) };
}

/** Approximate character budget for `text-xs` mono in a given pixel width. */
export function charsForMonoWidth(widthPx: number): number {
  return Math.max(0, Math.floor(widthPx / 7));
}
