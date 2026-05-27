/** Per-file diff body collapse in the flat diff view (keyed by file path). */
class DiffFileCollapseStore {
  collapsed = $state<ReadonlySet<string>>(new Set());
  /** Bumped on every mutation so derived models reliably re-run. */
  revision = $state(0);

  isCollapsed(filePath: string): boolean {
    return this.collapsed.has(filePath);
  }

  private commit(next: ReadonlySet<string>) {
    this.collapsed = next;
    this.revision++;
  }

  toggle(filePath: string) {
    const next = new Set(this.collapsed);
    if (next.has(filePath)) next.delete(filePath);
    else next.add(filePath);
    this.commit(next);
  }

  collapse(filePath: string) {
    if (this.collapsed.has(filePath)) return;
    const next = new Set(this.collapsed);
    next.add(filePath);
    this.commit(next);
  }

  expand(filePath: string) {
    if (!this.collapsed.has(filePath)) return;
    const next = new Set(this.collapsed);
    next.delete(filePath);
    this.commit(next);
  }

  collapseAll(paths: readonly string[]) {
    if (paths.length === 0) return;
    const next = new Set(this.collapsed);
    for (const p of paths) next.add(p);
    this.commit(next);
  }

  expandAll() {
    if (this.collapsed.size === 0) return;
    this.commit(new Set());
  }

  clear() {
    if (this.collapsed.size === 0) return;
    this.commit(new Set());
  }
}

export const diffFileCollapse = new DiffFileCollapseStore();
