import type { Meta, StoryObj } from "@storybook/svelte";
import ReviewPageSwitchHarness from "./ReviewPageSwitchHarness.svelte";
import { richSnapshot } from "./fixtures";
import type { AppSnapshot, FileSnapshot, HunkSnapshot, LineSnapshot } from "$lib/types";

/**
 * Overview → Diff with the diff kept mounted under the Overview, against the
 * old remount. "Measure" reports the median time to the second frame after
 * the switch, so the two stories give the before and after for the same diff.
 */
const meta = {
  title: "Shell/Review page switch",
  component: ReviewPageSwitchHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof ReviewPageSwitchHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

function line(kind: LineSnapshot["kind"], n: number, text: string): LineSnapshot {
  return {
    old_num: kind === "add" ? null : n,
    new_num: kind === "del" ? null : n,
    kind,
    text,
    spans: [{ text, color: "" }],
  };
}

function hunk(path: string, start: number): HunkSnapshot {
  const lines: LineSnapshot[] = [];
  for (let i = 0; i < 24; i++) {
    const kind = i % 6 === 2 ? "add" : i % 6 === 4 ? "del" : "context";
    lines.push(line(kind, start + i, `  const value${i} = compute("${path}", ${start + i});`));
  }
  return {
    header: `@@ -${start},24 +${start},24 @@`,
    old_start: start,
    old_count: 24,
    new_start: start,
    new_count: 24,
    lines,
    threads: [],
  };
}

function file(idx: number): FileSnapshot {
  const path = `src/review/module-${String(idx).padStart(2, "0")}.ts`;
  return {
    path,
    status: "modified",
    additions: 8,
    deletions: 8,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks: [hunk(path, 10), hunk(path, 80), hunk(path, 160)],
    source_index: idx,
    cache_key: `k${idx}`,
  };
}

const files = Array.from({ length: 60 }, (_, i) => file(i));
const snapshot: AppSnapshot = {
  ...richSnapshot,
  files,
  selected_file: 0,
  total_count: files.length,
  reviewed_count: 0,
};

export const KeptMounted: Story = { args: { snapshot, keepMounted: true } };

export const Remounted: Story = { args: { snapshot, keepMounted: false } };

export const KeptMountedLight: Story = {
  args: { snapshot: { ...snapshot, theme: "daylight" }, keepMounted: true },
};
