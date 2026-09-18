import type { ImportanceFileSnapshot } from "./types";

/** How many resolved files the settings card lists before it stops counting.
 *
 * A large diff resolves to hundreds of rows, which would push the rules table
 * the card is about off the page. The remainder is counted rather than dropped,
 * so a short list never reads as a complete one.
 */
export const IMPORTANCE_FILE_LIMIT = 40;

export interface ImportanceFileWindow {
  shown: ImportanceFileSnapshot[];
  hidden: number;
}

export function importanceFileWindow(
  files: ImportanceFileSnapshot[],
  limit: number = IMPORTANCE_FILE_LIMIT,
): ImportanceFileWindow {
  if (files.length <= limit) {
    return { shown: files, hidden: 0 };
  }
  return { shown: files.slice(0, limit), hidden: files.length - limit };
}

/** What the card names as the source of a file's tier.
 *
 * A file no rule claimed shows the default's tier and no rule key, so the
 * absent case needs words of its own — an empty cell reads as missing data
 * rather than as an answer.
 */
export function matchedRuleLabel(file: ImportanceFileSnapshot): string {
  return file.matchedRule ?? "no rule";
}
