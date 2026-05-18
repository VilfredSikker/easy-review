import { describe, expect, it } from "bun:test";
import { buildTree, flattenForNav } from "./treeFromPaths";
import type { FileSnapshot } from "./types";

function file(path: string): FileSnapshot {
  // Only `path` matters for tree shape; cast satisfies the test.
  return { path } as unknown as FileSnapshot;
}

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
