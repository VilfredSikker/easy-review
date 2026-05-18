import type { Meta, StoryObj } from "@storybook/svelte";
import DiffViewHarness from "./DiffViewHarness.svelte";
import { richSnapshot, commentThread } from "./fixtures";
import type {
  AppSnapshot,
  FileSnapshot,
  HunkSnapshot,
  LineSnapshot,
} from "$lib/types";

const meta = {
  title: "Diff/DiffView",
  component: DiffViewHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof DiffViewHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Unified: Story = {
  args: { snapshot: richSnapshot, viewModeOverride: "unified" },
};

export const Split: Story = {
  args: { snapshot: richSnapshot, viewModeOverride: "split" },
};

/**
 * Split mode with an inline comment thread anchored to a line in the first
 * hunk. The thread row spans both sides.
 */
/**
 * Word-diff highlights — a hunk with clear modify pairs (variable rename,
 * string-literal change, signature tweak). In split mode the differing
 * tokens get a darker background on each side; the rest of the line keeps
 * the standard add/del tint.
 */
export const WordDiff: Story = {
  args: (() => {
    const ctx = (o: number, n: number, t: string): LineSnapshot => ({
      old_num: o, new_num: n, kind: "context", spans: [{ text: t, color: "" }],
    });
    const del = (o: number, t: string): LineSnapshot => ({
      old_num: o, new_num: null, kind: "del", spans: [{ text: t, color: "" }],
    });
    const add = (n: number, t: string): LineSnapshot => ({
      old_num: null, new_num: n, kind: "add", spans: [{ text: t, color: "" }],
    });
    const hunk: HunkSnapshot = {
      header: "@@ -1,8 +1,8 @@ word-diff demo",
      old_start: 1, old_count: 8, new_start: 1, new_count: 8,
      lines: [
        ctx(1, 1, "function greet(user) {"),
        del(2, '  const message = "Hello, " + user.name;'),
        add(2, '  const greeting = "Hi there, " + user.fullName;'),
        del(3, "  console.log(message);"),
        add(3, "  console.log(greeting);"),
        ctx(4, 4, "}"),
        ctx(5, 5, ""),
        del(6, "export function totalPrice(items, taxRate) {"),
        add(6, "export function totalPrice(items, taxRate, discount) {"),
        del(7, "  return items.reduce((a, b) => a + b.price, 0) * (1 + taxRate);"),
        add(7, "  return items.reduce((a, b) => a + b.price, 0) * (1 + taxRate) - discount;"),
        ctx(8, 8, "}"),
      ],
      threads: [],
    };
    const file: FileSnapshot = {
      path: "src/example/word-diff.ts",
      status: "modified",
      additions: 4, deletions: 4,
      reviewed: false, compacted: false, risk: null,
      finding_count: 0, comment_count: 0, question_count: 0,
      hunks: [hunk],
      source_index: 0,
    };
    const snap: AppSnapshot = JSON.parse(JSON.stringify(richSnapshot));
    snap.files = [file];
    snap.selected_file = 0;
    snap.total_count = 1;
    snap.reviewed_count = 0;
    return { snapshot: snap, viewModeOverride: "split" as const };
  })(),
};

export const WordDiffUnified: Story = {
  args: (() => {
    const ctx = (o: number, n: number, t: string): LineSnapshot => ({
      old_num: o, new_num: n, kind: "context", spans: [{ text: t, color: "" }],
    });
    const del = (o: number, t: string): LineSnapshot => ({
      old_num: o, new_num: null, kind: "del", spans: [{ text: t, color: "" }],
    });
    const add = (n: number, t: string): LineSnapshot => ({
      old_num: null, new_num: n, kind: "add", spans: [{ text: t, color: "" }],
    });
    const hunk: HunkSnapshot = {
      header: "@@ -1,6 +1,6 @@ word-diff demo (unified)",
      old_start: 1, old_count: 6, new_start: 1, new_count: 6,
      lines: [
        ctx(1, 1, "function greet(user) {"),
        del(2, '  const message = "Hello, " + user.name;'),
        add(2, '  const greeting = "Hi there, " + user.fullName;'),
        del(3, "  console.log(message);"),
        add(3, "  console.log(greeting);"),
        ctx(4, 4, "}"),
      ],
      threads: [],
    };
    const file: FileSnapshot = {
      path: "src/example/word-diff.ts",
      status: "modified",
      additions: 2, deletions: 2,
      reviewed: false, compacted: false, risk: null,
      finding_count: 0, comment_count: 0, question_count: 0,
      hunks: [hunk],
      source_index: 0,
    };
    const snap: AppSnapshot = JSON.parse(JSON.stringify(richSnapshot));
    snap.files = [file];
    snap.selected_file = 0;
    snap.total_count = 1;
    snap.reviewed_count = 0;
    return { snapshot: snap, viewModeOverride: "unified" as const };
  })(),
};

/**
 * Long-line content that exceeds viewport width — exercises horizontal scroll
 * on the diff body. Verifies that add/del backgrounds extend as a single
 * unified-width block across all rows, not per-row, when scrolling right.
 */
export const LongLinesHorizontalScroll: Story = {
  args: (() => {
    const ctx = (o: number, n: number, t: string): LineSnapshot => ({
      old_num: o, new_num: n, kind: "context", spans: [{ text: t, color: "" }],
    });
    const del = (o: number, t: string): LineSnapshot => ({
      old_num: o, new_num: null, kind: "del", spans: [{ text: t, color: "" }],
    });
    const add = (n: number, t: string): LineSnapshot => ({
      old_num: null, new_num: n, kind: "add", spans: [{ text: t, color: "" }],
    });
    const veryLong =
      "const veryLongIdentifierName = await some.deeply.nested.module.invokeRemoteProcedureWithManyParameters(firstArgument, secondArgument, thirdArgument, fourthArgument, fifthArgument, sixthArgument, seventhArgument, eighthArgument);";
    const veryLong2 =
      "const renamedVeryLongIdentifierName = await some.deeply.nested.module.invokeRemoteProcedureWithManyParameters(firstArgument, secondArgument, thirdArgument, fourthArgument, fifthArgument, sixthArgument, seventhArgument, eighthArgument, ninthArgument);";
    const hunk: HunkSnapshot = {
      header: "@@ -1,9 +1,9 @@ long-line horizontal scroll demo",
      old_start: 1, old_count: 9, new_start: 1, new_count: 9,
      lines: [
        ctx(1, 1, "function processBatch(items) {"),
        ctx(2, 2, "  const results = [];"),
        ctx(3, 3, "  for (const item of items) {"),
        del(4, "    " + veryLong),
        add(4, "    " + veryLong2),
        ctx(5, 5, "    results.push(result);"),
        ctx(6, 6, "  }"),
        del(7, "  return results.filter((r) => r !== null && r.status === 'ok' && r.value > 0);"),
        add(7, "  return results.filter((r) => r !== null && r.status === 'ok' && r.value > 0 && r.timestamp > Date.now() - 86400000);"),
        ctx(8, 8, "}"),
        ctx(9, 9, "// trailing short context line — should still align with widest row's background"),
      ],
      threads: [],
    };
    const file: FileSnapshot = {
      path: "src/example/long-lines.ts",
      status: "modified",
      additions: 2, deletions: 2,
      reviewed: false, compacted: false, risk: null,
      finding_count: 0, comment_count: 0, question_count: 0,
      hunks: [hunk],
      source_index: 0,
    };
    const snap: AppSnapshot = JSON.parse(JSON.stringify(richSnapshot));
    snap.files = [file];
    snap.selected_file = 0;
    snap.total_count = 1;
    snap.reviewed_count = 0;
    return { snapshot: snap, viewModeOverride: "unified" as const };
  })(),
};

export const SplitWithThreads: Story = {
  args: (() => {
    const snap: AppSnapshot = JSON.parse(JSON.stringify(richSnapshot));
    // Attach an extra thread targeted at an `add` line in the first hunk so it
    // visibly renders between two split rows.
    const firstFile = snap.files[0];
    if (firstFile && firstFile.hunks[0]) {
      firstFile.hunks[0].threads = [
        ...firstFile.hunks[0].threads,
        { ...commentThread, id: "thread-split-demo", line: 39 },
      ];
    }
    return { snapshot: snap, viewModeOverride: "split" as const };
  })(),
};
