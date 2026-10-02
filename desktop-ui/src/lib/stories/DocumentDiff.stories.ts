import type { Meta, StoryObj } from "@storybook/svelte";
import DocumentDiffHarness from "./DocumentDiffHarness.svelte";
import { richSnapshot, fileMediaCombobox, commentThread } from "./fixtures";
import type { AppSnapshot, FileSnapshot } from "$lib/types";

const meta = {
  title: "Diff/Document previews",
  component: DocumentDiffHarness,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof DocumentDiffHarness>;
export default meta;
type Story = StoryObj<typeof meta>;

const markdown = `# Document preview

This is the complete document. **Strong text**, *emphasis*, and ~~removed text~~.

| Mode | Behavior |
| --- | --- |
| Raw | Diff lines |
| Preview | Full document |

- [x] Read the document
- [ ] Review the diff

[External link](https://example.com) and [repository link](./README.md).

![Remote image](https://images.unsplash.com/photo-1441974231531-c6227db76b6e?w=800)

${Array.from({ length: 16 }, (_, i) => `## Section ${i + 1}\n\nA paragraph in the resulting document. The diff only contains the changed lines.`).join("\n\n")}
`;
const text = "Plain text preserves whitespace.\n\n    Indented text\n\tTabbed text\n\n" + "A long line wraps within the full diff column. ".repeat(18);
function documentFile(path: string, idx: number): FileSnapshot {
  return {
    ...fileMediaCombobox,
    path,
    source_index: idx,
    preview_key: `storybook-document-${idx}`,
    hunks: [{
      header: "@@ -1 +1 @@", old_start: 1, old_count: 1, new_start: 1, new_count: 1,
      threads: [],
      lines: [
        { kind: "del", old_num: 1, new_num: null, text: "Previous document", spans: [] },
        { kind: "add", old_num: null, new_num: 1, text: "Document preview", spans: [] },
      ],
    }],
  };
}
const files = [documentFile("docs/README.MD", 0), { ...fileMediaCombobox, source_index: 1 }, documentFile("notes.text", 2), documentFile("empty.md", 3), documentFile("unavailable.md", 4)];
const documentThread = { ...commentThread, id: "document-thread", file: "docs/README.MD", line: 1, replies: [], root: { ...commentThread.root, id: "document-thread", body_markdown: "Document review comment" } };
const documentFinding = { ...richSnapshot.ai.findings[0], id: "document-finding", file: "docs/README.MD", line: 1, hunk_index: 0, title: "Document review finding", thread_id: null };
const snapshot: AppSnapshot = {
  ...richSnapshot,
  preview_context_key: "storybook-document-context",
  files: files.map((f) => f.path === documentThread.file ? { ...f, hunks: f.hunks.map((h) => ({ ...h, threads: [documentThread] })) } : f),
  ai: { ...richSnapshot.ai, threads: [documentThread], findings: [documentFinding] },
  selected_file: 0,
  total_count: files.length,
};
const documents = { "docs/README.MD": markdown, "notes.text": text, "empty.md": "" };
export const Unified: Story = { args: { snapshot, documents, viewModeOverride: "unified" } };
export const Split: Story = { args: { snapshot, documents, viewModeOverride: "split" } };
export const Guide: Story = {
  args: {
    snapshot: {
      ...snapshot,
      mode: "tour",
      tour: {
        title: "Document review",
        overviewMarkdown: "Read the full document before reviewing the diff.",
        available: true,
        scope: "branch",
        fresh: true,
        pillars: [{ id: "documents", title: "Documents", descriptionMarkdown: "Read complete documents beside the code diff.", importance: 1, foundation: false, reviewedCount: 0, totalCount: 2, files: [{ path: "docs/README.MD", reason: "Read the complete document.", findingIds: [] }, { path: "notes.text", reason: "Read the plain text.", findingIds: [] }] }],
      },
    },
    documents,
    viewModeOverride: "unified",
  },
};

const shortFiles = Array.from({ length: 24 }, (_, i) => documentFile(`short-${i}.txt`, i));
export const ManyShortDocuments: Story = {
  args: {
    snapshot: { ...snapshot, files: shortFiles, total_count: shortFiles.length, ai: { ...snapshot.ai, threads: [], findings: [] } },
    documents: Object.fromEntries(shortFiles.map((file, i) => [file.path, `Complete short document ${i}`])),
    viewModeOverride: "unified",
  },
};
