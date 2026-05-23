import type { Meta, StoryObj } from "@storybook/svelte";
import TreesFileTreeSpikeHarness from "$lib/spikes/TreesFileTreeSpikeHarness.svelte";
import { multiFolderSnapshot, richSnapshot } from "./fixtures";
import type { AppSnapshot, FileSnapshot } from "$lib/types";

function largeSnapshot(fileCount = 1200): AppSnapshot {
  const files: FileSnapshot[] = [];
  for (let i = 0; i < fileCount; i++) {
    const dir = i % 5 === 0 ? "packages/api/src" : i % 3 === 0 ? "apps/web/routes" : "lib";
    files.push({
      path: `${dir}/module-${i}.ts`,
      status: i % 7 === 0 ? "added" : i % 11 === 0 ? "deleted" : "modified",
      additions: (i * 3) % 40,
      deletions: (i * 2) % 15,
      reviewed: i % 20 === 0,
      compacted: false,
      risk: i % 13 === 0 ? "high" : i % 17 === 0 ? "med" : null,
      finding_count: i % 19 === 0 ? 2 : 0,
      comment_count: i % 23 === 0 ? 1 : 0,
      question_count: 0,
      hunks: [],
      source_index: i,
      cache_key: `large-${i}`,
      is_lazy_stub: i % 31 === 0,
    });
  }
  return {
    ...multiFolderSnapshot,
    files,
    total_count: files.length,
    reviewed_count: files.filter((f) => f.reviewed).length,
  };
}

const meta = {
  title: "Spikes/TreesSoftware",
  component: TreesFileTreeSpikeHarness,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof TreesFileTreeSpikeHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Typical PR file list with git status + row decorations. */
export const Default: Story = {
  args: { snapshot: multiFolderSnapshot },
};

/** Richer fixture: findings, comments, mixed statuses. */
export const RichPr: Story = {
  args: { snapshot: richSnapshot },
};

/** Stress virtualized window + preparedInput (~1.2k paths). */
export const LargeRepo: Story = {
  args: { snapshot: largeSnapshot(1200) },
};

/** er FileTree vs trees.software side by side; use “Simulate watch” on the right. */
export const CompareWithEr: Story = {
  args: { snapshot: multiFolderSnapshot, compareEr: true },
};
