/** Per-file diff body collapse in the flat diff view (keyed by file path). */
let collapsed = $state<ReadonlySet<string>>(new Set());
/** Bumped on every mutation so derived models reliably re-run. */
let revision = $state(0);

function commit(next: ReadonlySet<string>) {
  collapsed = next;
  revision++;
}

function isCollapsed(filePath: string): boolean {
  return collapsed.has(filePath);
}

function toggle(filePath: string) {
  const next = new Set(collapsed);
  if (next.has(filePath)) next.delete(filePath);
  else next.add(filePath);
  commit(next);
}

function collapse(filePath: string) {
  if (collapsed.has(filePath)) return;
  const next = new Set(collapsed);
  next.add(filePath);
  commit(next);
}

function expand(filePath: string) {
  if (!collapsed.has(filePath)) return;
  const next = new Set(collapsed);
  next.delete(filePath);
  commit(next);
}

function collapseAll(paths: readonly string[]) {
  if (paths.length === 0) return;
  const next = new Set(collapsed);
  for (const p of paths) next.add(p);
  commit(next);
}

function expandAll() {
  if (collapsed.size === 0) return;
  commit(new Set());
}

function clear() {
  if (collapsed.size === 0) return;
  commit(new Set());
}

export const diffFileCollapse = {
  get collapsed() {
    return collapsed;
  },
  get revision() {
    return revision;
  },
  isCollapsed,
  toggle,
  collapse,
  expand,
  collapseAll,
  expandAll,
  clear,
};
