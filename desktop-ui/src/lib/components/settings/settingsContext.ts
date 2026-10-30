import { getContext, setContext } from "svelte";

/**
 * The settings search reaches every row through context rather than props: rows
 * sit several components deep under hand-built tab layouts, and threading a
 * query through each would make every layout a search component too.
 *
 * Each `use*` reader must run during component init, like any `getContext`.
 */
const QUERY_KEY = Symbol("settings-query");
const SECTION_KEY = Symbol("settings-section-matched");

export function provideSettingsQuery(get: () => string): void {
  setContext(QUERY_KEY, get);
}

export function useSettingsQuery(): () => string {
  return getContext<(() => string) | undefined>(QUERY_KEY) ?? (() => "");
}

/** A section whose title matches the query shows every row it holds. */
export function provideSectionMatched(get: () => boolean): void {
  setContext(SECTION_KEY, get);
}

export function useSectionMatched(): () => boolean {
  return getContext<(() => boolean) | undefined>(SECTION_KEY) ?? (() => false);
}
