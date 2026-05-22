import type { Meta, StoryObj } from "@storybook/svelte";
import DiffViewHarness from "./DiffViewHarness.svelte";
import {
  fileMediaCombobox,
  fileVariantWarningCopy,
  fileExperimentTemplate,
  richSnapshot,
} from "./fixtures";
import type { AppSnapshot, FileSnapshot, HunkSnapshot, LineSnapshot } from "$lib/types";

const meta = {
  title: "Diff/DiffView (continuous)",
  component: DiffViewHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof DiffViewHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

// Build a small hunk for the otherwise hunk-less fixture files so the
// continuous-scroll story has real content for all three files.
function makeHunk(label: string, startLine: number): HunkSnapshot {
  const lines: LineSnapshot[] = [
    { old_num: startLine, new_num: startLine, kind: "context", text: `// ${label} — context`, spans: [{ text: `// ${label} — context`, color: "" }] },
    { old_num: null, new_num: startLine + 1, kind: "add", text: `  added line in ${label}`, spans: [{ text: `  added line in ${label}`, color: "" }] },
    { old_num: startLine + 1, new_num: null, kind: "del", text: `  removed line in ${label}`, spans: [{ text: `  removed line in ${label}`, color: "" }] },
    { old_num: startLine + 2, new_num: startLine + 2, kind: "context", text: `// trailing context`, spans: [{ text: `// trailing context`, color: "" }] },
  ];
  return {
    header: `@@ -${startLine},3 +${startLine},3 @@ ${label}`,
    old_start: startLine,
    old_count: 3,
    new_start: startLine,
    new_count: 3,
    lines,
    threads: [],
  };
}

const variantWithHunks: FileSnapshot = {
  ...fileVariantWarningCopy,
  hunks: [makeHunk("variant-warning", 40), makeHunk("variant-warning #2", 100)],
};
const templateWithHunks: FileSnapshot = {
  ...fileExperimentTemplate,
  hunks: [makeHunk("experiment-template", 12)],
};

/** Three files in one continuous scroll — sticky per-file headers, hunks visible for each. */
export const ThreeFileScroll: Story = {
  args: (() => {
    const snap: AppSnapshot = {
      ...richSnapshot,
      files: [fileMediaCombobox, variantWithHunks, templateWithHunks],
      selected_file: 0,
      total_count: 3,
      reviewed_count: 0,
    };
    return { snapshot: snap, viewModeOverride: "unified" as const };
  })(),
};

/**
 * 30-file scroll for exercising the per-file virtualization. Off-screen files
 * render a height-estimated stub, mount lazily when they enter the ±1
 * viewport window, and un-mount once they leave it.
 */
function makeBigFile(idx: number): FileSnapshot {
  const path = `src/generated/file-${String(idx).padStart(2, "0")}.ts`;
  return {
    path,
    status: "modified",
    additions: 3,
    deletions: 1,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks: [makeHunk(`file-${idx}`, 10), makeHunk(`file-${idx} #2`, 60)],
    source_index: idx,
    cache_key: "",
  };
}

export const LargeDiff: Story = {
  args: (() => {
    const files: FileSnapshot[] = [];
    for (let i = 0; i < 30; i++) files.push(makeBigFile(i));
    const snap: AppSnapshot = {
      ...richSnapshot,
      files,
      selected_file: 0,
      total_count: files.length,
      reviewed_count: 0,
    };
    return { snapshot: snap, viewModeOverride: "unified" as const };
  })(),
};
