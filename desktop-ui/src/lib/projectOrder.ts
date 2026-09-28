/// `items` rearranged to follow `order` (ids), for showing a drag-reorder
/// before the backend confirms it. Ids missing from `items` are skipped, and
/// items `order` does not mention keep their relative order at the end.
export function orderedByIds<T extends { id: string }>(items: readonly T[], order: readonly string[]): T[] {
  const byId = new Map(items.map((p) => [p.id, p]));
  const out: T[] = [];
  for (const id of order) {
    const p = byId.get(id);
    if (p) {
      out.push(p);
      byId.delete(id);
    }
  }
  for (const p of items) {
    if (byId.has(p.id)) out.push(p);
  }
  return out;
}
