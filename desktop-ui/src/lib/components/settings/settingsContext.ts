import { getContext, setContext } from "svelte";

/**
 * The settings search reaches every row through context rather than props: rows
 * sit several components deep under hand-built tab layouts, and threading a
 * query through each would make every layout a search component too.
 *
 * Each `use*` reader must run during component init, like any `getContext`.
 */
const QUERY_KEY = Symbol("settings-query");
const SECTION_KEY = Symbol("settings-section-search");

export function provideSettingsQuery(get: () => string): void {
  setContext(QUERY_KEY, get);
}

export function useSettingsQuery(): () => string {
  return getContext<(() => string) | undefined>(QUERY_KEY) ?? (() => "");
}

/**
 * How a section and its rows settle the search between them. A row reports
 * whether its own text matches, and the section shows while any row does, so
 * a section is never visible with every row hidden. `showAll` is the reverse:
 * the section's title, or text it renders outside any row, matched, so every
 * row shows.
 */
export interface SectionSearch {
  showAll: () => boolean;
  report: (row: symbol, matched: boolean) => void;
  drop: (row: symbol) => void;
}

export function provideSectionSearch(search: SectionSearch): void {
  setContext(SECTION_KEY, search);
}

export function useSectionSearch(): SectionSearch | undefined {
  return getContext<SectionSearch | undefined>(SECTION_KEY);
}
