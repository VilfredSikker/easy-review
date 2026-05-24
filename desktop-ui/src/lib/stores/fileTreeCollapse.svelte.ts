/** Folder paths (collapsed-chain `fullPath`) hidden by the user in the file tree. */
class FileTreeCollapseStore {
  collapsed = $state<ReadonlySet<string>>(new Set());

  isCollapsed(folderPath: string): boolean {
    return this.collapsed.has(folderPath);
  }

  toggle(folderPath: string) {
    const next = new Set(this.collapsed);
    if (next.has(folderPath)) next.delete(folderPath);
    else next.add(folderPath);
    this.collapsed = next;
  }

  /** Expand every ancestor folder row for `filePath`. */
  expandAncestorsOf(filePath: string) {
    if (!filePath.includes("/")) return;
    const next = new Set(this.collapsed);
    let changed = false;
    for (const folderPath of this.collapsed) {
      if (filePath.startsWith(`${folderPath}/`)) {
        next.delete(folderPath);
        changed = true;
      }
    }
    if (changed) this.collapsed = next;
  }

  expandAll() {
    if (this.collapsed.size === 0) return;
    this.collapsed = new Set();
  }
}

export const fileTreeCollapse = new FileTreeCollapseStore();
