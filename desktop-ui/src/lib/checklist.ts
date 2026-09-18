import type { ChecklistItemSnapshot } from "./types";

/**
 * The categories the general-review prompt asks for, in the order an
 * outcome-shaped checklist reads best: what the schema now says, what proves
 * it, what callers see, whether the auth path moved, and whether this is the
 * change that was actually asked for.
 *
 * Anything else — a free-form category, or the empty string a checklist
 * generated before these existed carries — sorts after these, keeping the
 * order the file already has.
 */
export const CHECKLIST_CATEGORY_ORDER: readonly string[] = [
  "schema",
  "tests",
  "api",
  "auth",
  "plan",
];

const CATEGORY_LABELS: Record<string, string> = {
  schema: "Schema",
  tests: "Tests",
  api: "API",
  auth: "Auth",
  plan: "Plan",
};

/** One category's rows. `index` is the item's position in the flat list. */
export interface ChecklistGroup {
  category: string;
  label: string;
  rows: { index: number; item: ChecklistItemSnapshot }[];
}

/** Group rows by category without reordering within a category. */
export function groupChecklistItems(items: ChecklistItemSnapshot[]): ChecklistGroup[] {
  const byCategory = new Map<string, ChecklistGroup>();
  items.forEach((item, index) => {
    const category = item.category.trim();
    let group = byCategory.get(category);
    if (!group) {
      group = { category, label: checklistCategoryLabel(category), rows: [] };
      byCategory.set(category, group);
    }
    group.rows.push({ index, item });
  });

  const rank = (category: string): number => {
    const known = CHECKLIST_CATEGORY_ORDER.indexOf(category);
    return known === -1 ? CHECKLIST_CATEGORY_ORDER.length : known;
  };
  // Stable sort: unknown categories keep the order the generator wrote them in.
  return [...byCategory.values()].sort((a, b) => rank(a.category) - rank(b.category));
}

/** "Schema", or the raw category when it is not one of the five. */
export function checklistCategoryLabel(category: string): string {
  if (!category) return "Other";
  return CATEGORY_LABELS[category] ?? category;
}

/** "4/7" for the card header, or "0/0" for an empty checklist. */
export function checklistProgress(items: ChecklistItemSnapshot[]): string {
  return `${items.filter((i) => i.checked).length}/${items.length}`;
}
