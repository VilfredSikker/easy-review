// Persisted order for sidebar sections within a project. Global — one order
// applies to all projects. Stored in localStorage like other UI prefs.

const STORAGE_KEY = "sidebarSectionOrder";

export type SidebarSection =
  | "saved"
  | "tracked"
  | "my_prs"
  | "to_review"
  | "recent"
  | "recently_merged";

export const DEFAULT_ORDER: SidebarSection[] = [
  "saved",
  "tracked",
  "my_prs",
  "to_review",
  "recent",
  "recently_merged",
];

const VALID = new Set<string>(DEFAULT_ORDER);

function loadInitial(): SidebarSection[] {
  if (typeof localStorage === "undefined") return [...DEFAULT_ORDER];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [...DEFAULT_ORDER];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [...DEFAULT_ORDER];
    const filtered = parsed.filter((s: string) => VALID.has(s));
    // Add any missing sections (forward-compat if new sections are added)
    for (const s of DEFAULT_ORDER) {
      if (!filtered.includes(s)) filtered.push(s);
    }
    return filtered as SidebarSection[];
  } catch {
    return [...DEFAULT_ORDER];
  }
}

function persist(order: SidebarSection[]) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(order));
  } catch {
    /* ignore quota / privacy-mode errors */
  }
}

function createSectionOrderStore() {
  let order = $state<SidebarSection[]>(loadInitial());

  return {
    get order() {
      return order;
    },
    set(next: SidebarSection[]) {
      order = next;
      persist(next);
    },
  };
}

export const sectionOrder = createSectionOrderStore();
