import { describe, expect, it } from "bun:test";
import {
  buildTree,
  filesByPathMap,
  flattenForNav,
  formatFolderLabel,
  resolveTreeFile,
  visibleTree,
} from "./treeFromPaths";
import type { FileSnapshot } from "./types";

function file(path: string, overrides: Partial<FileSnapshot> = {}): FileSnapshot {
  return {
    path,
    status: "modified",
    additions: 0,
    deletions: 0,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks: [],
    source_index: 0,
    cache_key: path,
    ...overrides,
  };
}

describe("formatFolderLabel", () => {
  it("joins segments with ›", () => {
    expect(formatFolderLabel(["a", "b"])).toBe("a › b");
    expect(formatFolderLabel(["packages", "discovery-platform", "src"])).toBe(
      "packages › discovery-platform › src",
    );
  });

  it("ellipsis middle when 4+ segments", () => {
    expect(formatFolderLabel(["admin", "lib", "auth", "middleware"])).toBe(
      "admin › … › middleware",
    );
  });
});

describe("flattenForNav", () => {
  it("returns empty for empty input", () => {
    expect(flattenForNav(buildTree([]))).toEqual([]);
  });

  it("renders routes folder in visual order (folders-before-files)", () => {
    // Mirrors the bug repro: lexicographic order puts
    // `organizations/+page.svelte` BEFORE the nested `[id]/`, `new/` siblings,
    // but the tree (folders first, then files) renders it LAST inside
    // organizations/. So the visual neighbor of `users/+page.server.ts` is
    // `organizations/+page.svelte`.
    const paths = [
      "routes/organizations/+page.svelte",
      "routes/organizations/[id]/+page.server.ts",
      "routes/organizations/[id]/+page.svelte",
      "routes/organizations/new/+page.server.ts",
      "routes/organizations/new/+page.svelte",
      "routes/organizations/new/__tests__/page.server.test.ts",
      "routes/users/+page.server.ts",
      "routes/users/+page.svelte",
    ];
    const order = flattenForNav(buildTree(paths.map(file)));

    // organizations/+page.svelte should immediately precede users/+page.server.ts.
    const orgIdx = order.indexOf("routes/organizations/+page.svelte");
    const userIdx = order.indexOf("routes/users/+page.server.ts");
    expect(orgIdx).toBeGreaterThanOrEqual(0);
    expect(userIdx).toBe(orgIdx + 1);

    // Full visual order: [id]/ files, then new/__tests__/ file, then new/ files,
    // then organizations/ own files, then users/ files.
    expect(order).toEqual([
      "routes/organizations/[id]/+page.server.ts",
      "routes/organizations/[id]/+page.svelte",
      "routes/organizations/new/__tests__/page.server.test.ts",
      "routes/organizations/new/+page.server.ts",
      "routes/organizations/new/+page.svelte",
      "routes/organizations/+page.svelte",
      "routes/users/+page.server.ts",
      "routes/users/+page.svelte",
    ]);
  });

  it("resolveTreeFile reads fresh stats when buildTree cache is stale", () => {
    const first = [file("src/a.ts", { additions: 1, deletions: 2 })];
    const tree = buildTree(first);
    const node = tree.find((n) => n.kind === "file" && n.fullPath === "src/a.ts");
    expect(node).toBeDefined();

    const second = [file("src/a.ts", { additions: 99, deletions: 0 })];
    const cachedAgain = buildTree(second);
    expect(cachedAgain).toBe(tree);

    const stale = resolveTreeFile(filesByPathMap(first), node!);
    expect(stale?.additions).toBe(1);

    const fresh = resolveTreeFile(filesByPathMap(second), node!);
    expect(fresh?.additions).toBe(99);
    expect(fresh?.deletions).toBe(0);
  });

  it("visibleTree hides descendants of collapsed folders", () => {
    const paths = [
      "src/lib/a.ts",
      "src/lib/nested/b.ts",
      "src/other.ts",
    ];
    const tree = buildTree(paths.map(file));
    const libFolder = tree.find((n) => n.kind === "folder" && n.fullPath === "src/lib");
    expect(libFolder).toBeDefined();

    const collapsed = new Set(["src/lib"]);
    const visible = visibleTree(tree, collapsed);
    const filePaths = visible.filter((n) => n.kind === "file").map((n) => n.fullPath);
    expect(filePaths).not.toContain("src/lib/a.ts");
    expect(filePaths).not.toContain("src/lib/nested/b.ts");
    expect(filePaths).toContain("src/other.ts");
    expect(flattenForNav(tree, collapsed)).toEqual(["src/other.ts"]);
  });

  it("uses › in collapsed folder row names", () => {
    const paths = ["apps/admin/src/lib/auth/middleware.ts"];
    const tree = buildTree(paths.map(file));
    const folder = tree.find((n) => n.kind === "folder");
    expect(folder?.name).toBe("apps › … › auth");
  });

  it("handles mixed depths with folders sorted before files at each level", () => {
    const paths = [
      "README.md",
      "src/index.ts",
      "src/utils/format.ts",
      "src/lib/a.ts",
      "package.json",
    ];
    const order = flattenForNav(buildTree(paths.map(file)));
    // Top level: src/ (folder) comes before README.md and package.json (files).
    // Inside src/: lib/ before utils/ (folder order), then index.ts (file).
    expect(order).toEqual([
      "src/lib/a.ts",
      "src/utils/format.ts",
      "src/index.ts",
      "package.json",
      "README.md",
    ]);
  });
});
