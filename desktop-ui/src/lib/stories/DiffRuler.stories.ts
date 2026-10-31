import type { Meta, StoryObj } from "@storybook/svelte";
import DiffViewHarness from "./DiffViewHarness.svelte";
import { richSnapshot } from "./fixtures";
import type { AppSnapshot, FileSnapshot, HunkSnapshot, LineSnapshot } from "$lib/types";

const meta = {
  title: "Diff/File map ruler",
  component: DiffViewHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof DiffViewHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

function line(kind: LineSnapshot["kind"], o: number | null, n: number | null, text: string): LineSnapshot {
  return { old_num: o, new_num: n, kind, text, spans: [{ text, color: "" }] };
}

function hunk(label: string, start: number): HunkSnapshot {
  return {
    header: `@@ -${start},3 +${start},3 @@ ${label}`,
    old_start: start,
    old_count: 3,
    new_start: start,
    new_count: 3,
    lines: [
      line("context", start, start, `export function ${label.replace(/\W/g, "_")}() {`),
      line("del", start + 1, null, "  return legacyValue;"),
      line("add", null, start + 1, "  return computeValue({ strict: true });"),
      line("context", start + 2, start + 2, "}"),
    ],
    threads: [],
  };
}

const DIRS = ["src/lib/stores", "src/lib/components", "crates/er-engine/src", "docs/adr"];

function file(i: number): FileSnapshot {
  return {
    path: `${DIRS[i % DIRS.length]}/module-${String(i).padStart(2, "0")}.ts`,
    status: "modified",
    additions: 3,
    deletions: 3,
    reviewed: i % 5 === 1,
    compacted: false,
    risk: null,
    finding_count: i % 7 === 3 ? 1 : 0,
    comment_count: i % 6 === 2 ? 2 : 0,
    question_count: 0,
    hunks: [hunk(`module-${i}`, 10), hunk(`module-${i} b`, 40), hunk(`module-${i} c`, 80)],
    source_index: i,
    cache_key: "",
  };
}

/** Twenty-four files, some reviewed and some annotated. The ruler keeps its
 *  own strip left of the column, so its ticks never land on line numbers or
 *  code; scrolled into a file, the sticky header runs beside the strip, and
 *  the hovered list opens over that header rather than behind it. */
export const ManyFiles: Story = {
  args: (() => {
    const files = Array.from({ length: 24 }, (_, i) => file(i));
    const snap: AppSnapshot = {
      ...richSnapshot,
      files,
      selected_file: 0,
      total_count: files.length,
      reviewed_count: files.filter((f) => f.reviewed).length,
    };
    return { snapshot: snap, viewModeOverride: "unified" as const };
  })(),
};
