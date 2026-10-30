/** Timestamped names (migrations: `20261029093502_add_x.sql`) share a long
 *  numeric prefix, so lists lead with the part that differs — the descriptive
 *  name — and dim the time portion of the version after it. */
export interface ShortFileName {
  /** Shown first and at full weight. */
  main: string;
  /** Time part of the version, shown dimmed; empty for ordinary names. */
  stamp: string;
  /** Full version (date + time) for the tooltip; empty for ordinary names. */
  version: string;
}

const TIMESTAMPED = /^(\d{8})(\d{6})_(.+)$/;

/** Takes a basename; callers keep the directory for the full-path title. */
export function shortFileName(name: string): ShortFileName {
  const m = TIMESTAMPED.exec(name);
  if (!m) return { main: name, stamp: "", version: "" };
  return { main: m[3], stamp: m[2], version: `${m[1]}${m[2]}` };
}

export function baseName(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? path : path.slice(i + 1);
}
