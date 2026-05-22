import type { FileSnapshot } from "$lib/types";

/**
 * Renderable tree node. Folders carry no `file` reference; files do.
 * `depth` is the indentation level (0 = top-level row).
 */
export interface TreeNode {
  name: string;
  fullPath: string;
  depth: number;
  kind: "folder" | "file";
  file?: FileSnapshot;
  children?: TreeNode[];
}

interface MutableFolder {
  name: string;
  fullPath: string;
  children: Map<string, MutableFolder | MutableFile>;
}
interface MutableFile {
  name: string;
  fullPath: string;
  file: FileSnapshot;
}

function isFolder(n: MutableFolder | MutableFile): n is MutableFolder {
  return "children" in n;
}

/**
 * Build a folder tree from a flat list of files. VS Code-style collapsing:
 * single-child folder chains get joined into one row
 * (e.g. `packages/discovery-platform` instead of two nested rows).
 *
 * Returns a pre-order flat array of `TreeNode`s in render order, with `depth`
 * set so the caller can apply `pl-{depth*4}` (or similar) indentation.
 */
function _buildTree(files: FileSnapshot[]): TreeNode[] {
  // 1. Build a nested map of folders + files.
  const root: MutableFolder = { name: "", fullPath: "", children: new Map() };

  for (const file of files) {
    const segments = file.path.split("/").filter(Boolean);
    if (segments.length === 0) continue;
    let cursor = root;
    for (let i = 0; i < segments.length - 1; i++) {
      const seg = segments[i];
      const fullPath = segments.slice(0, i + 1).join("/");
      let next = cursor.children.get(seg);
      if (!next || !isFolder(next)) {
        next = { name: seg, fullPath, children: new Map() };
        cursor.children.set(seg, next);
      }
      cursor = next;
    }
    const leafName = segments[segments.length - 1];
    cursor.children.set(leafName, {
      name: leafName,
      fullPath: file.path,
      file,
    });
  }

  // 2. Walk the tree in pre-order, collapsing single-child folder chains.
  const out: TreeNode[] = [];

  const walk = (folder: MutableFolder, depth: number) => {
    // Sort: folders before files, then alphabetically.
    const entries = [...folder.children.values()].sort((a, b) => {
      const aFolder = isFolder(a);
      const bFolder = isFolder(b);
      if (aFolder !== bFolder) return aFolder ? -1 : 1;
      return a.name.localeCompare(b.name);
    });

    for (const entry of entries) {
      if (isFolder(entry)) {
        // Collapse single-child folder chains.
        const parts: string[] = [entry.name];
        let cur: MutableFolder = entry;
        while (cur.children.size === 1) {
          const only = [...cur.children.values()][0];
          if (!isFolder(only)) break;
          parts.push(only.name);
          cur = only;
        }
        out.push({
          name: parts.join("/"),
          fullPath: cur.fullPath,
          depth,
          kind: "folder",
        });
        walk(cur, depth + 1);
      } else {
        out.push({
          name: entry.name,
          fullPath: entry.fullPath,
          depth,
          kind: "file",
          file: entry.file,
        });
      }
    }
  };

  walk(root, 0);
  return out;
}

let _memoKey = "";
let _memoResult: TreeNode[] = [];

export function buildTree(files: FileSnapshot[]): TreeNode[] {
  const key = files.map((f) => f.path).join("\0");
  if (key === _memoKey) return _memoResult;
  _memoKey = key;
  _memoResult = _buildTree(files);
  return _memoResult;
}

/**
 * Returns leaf file paths in visual (depth-first, render) order — the same
 * order `FileTree.svelte` displays. Subfolders come before files within each
 * directory (alphabetical), matching `buildTree`'s sort.
 *
 * Used by keyboard navigation so `j`/`k` step through files in the order the
 * user sees them, not the path-sorted backend order.
 */
export function flattenForNav(tree: TreeNode[]): string[] {
  const out: string[] = [];
  for (const node of tree) {
    if (node.kind === "file") out.push(node.fullPath);
  }
  return out;
}
