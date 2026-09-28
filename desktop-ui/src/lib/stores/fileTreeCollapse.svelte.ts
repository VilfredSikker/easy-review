import { toggled, without } from "$lib/immutableSet";

/** Folder paths (collapsed-chain `fullPath`) hidden by the user in the file tree. */
class FileTreeCollapseStore {
  collapsed = $state<ReadonlySet<string>>(new Set());

  isCollapsed(folderPath: string): boolean {
    return this.collapsed.has(folderPath);
  }

  toggle(folderPath: string) {
    this.collapsed = toggled(this.collapsed, folderPath);
  }

  /** Expand every ancestor folder row for `filePath`. */
  expandAncestorsOf(filePath: string) {
    if (!filePath.includes("/")) return;
    const ancestors = [...this.collapsed].filter((folderPath) =>
      filePath.startsWith(`${folderPath}/`),
    );
    if (ancestors.length > 0) this.collapsed = without(this.collapsed, ancestors);
  }

  expandAll() {
    if (this.collapsed.size === 0) return;
    this.collapsed = new Set();
  }
}

export const fileTreeCollapse = new FileTreeCollapseStore();
