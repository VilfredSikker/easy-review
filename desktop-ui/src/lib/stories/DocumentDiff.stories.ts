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

\`\`\`mermaid
sequenceDiagram
  actor Dev as Developer
  participant A as Agent
  participant R as Reviewer
  Dev->>A: Describe the change
  A->>R: Push the diff
  R-->>Dev: Review notes
\`\`\`

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

const rfc = `# RFC 0002: Platform agent

The agent shows up at every stage of the value chain. Each stage sits in a different platform, built by a different team.

## Proposal

\`\`\`mermaid
flowchart LR
  Config[Platform config] --> Panel[Side panel]
  Panel --> Endpoint[Agent endpoint]
  Endpoint --> MCP[MCP server]
\`\`\`

The SDK has three parts:

- **UI**: a side panel the platform drops into its layout. It covers chat, streaming text, tool progress and approval prompts.
- **Server**: an endpoint the platform mounts in its own backend.
- **Config**: the only part a platform writes.

## Positions we propose

1. **One SDK, configured per platform.** Platforms write configuration and MCP tools.
2. **TypeScript, in its own repo.** The SDK ships as versioned packages.
3. **Data comes only through MCP.** The SDK never calls a platform database directly.

| Part | Owner |
| --- | --- |
| UI | SDK |
| Config | Platform |
`;
const rfcLines = rfc.split("\n");
const rfcFile: FileSnapshot = {
  ...documentFile("docs/rfd/0002-platform-agent/README.md", 0),
  additions: rfcLines.length,
  deletions: 0,
  hunks: [{
    header: `@@ -0,0 +1,${rfcLines.length} @@`, old_start: 0, old_count: 0, new_start: 1, new_count: rfcLines.length,
    threads: [],
    lines: rfcLines.map((lineText, i) => ({ kind: "add" as const, old_num: null, new_num: i + 1, text: lineText, spans: [] })),
  }],
};
const rfcThread = { ...documentThread, id: "rfc-thread", file: rfcFile.path, line: 17, root: { ...documentThread.root, id: "rfc-thread", body_markdown: "Should the server part own rate limits, or the platform?" } };
const rfcSnapshot: AppSnapshot = {
  ...snapshot,
  files: [{ ...rfcFile, hunks: rfcFile.hunks.map((h) => ({ ...h, threads: [rfcThread] })) }],
  total_count: 1,
  ai: { ...snapshot.ai, threads: [rfcThread], findings: [] },
};
export const SideBySide: Story = {
  args: { snapshot: rfcSnapshot, documents: { [rfcFile.path]: rfc }, viewModeOverride: "unified", documentMode: "side" },
};

const longDoc = Array.from({ length: 24 }, (_, i) => [
  `## Section ${i + 1}`,
  "",
  `A long paragraph for section ${i + 1} that wraps in the raw half and the rendered half at different widths. `.repeat(3).trim(),
  "",
  ...Array.from({ length: 6 }, (_item, j) => `- Item ${j + 1} of section ${i + 1}, with enough words to wrap on a narrow window`),
  "",
  ...(i % 6 === 2 ? ["| Part | Owner |", "| --- | --- |", "| UI | SDK |", "| Config | Platform |", ""] : []),
  ...(i % 8 === 5 ? ["```mermaid", "flowchart LR", "  A --> B", "  B --> C", "```", ""] : []),
].join("\n")).join("\n");
const longLines = longDoc.split("\n");
const longPath = "docs/long.md";
function sideFile(path: string, hunks: FileSnapshot["hunks"]): FileSnapshot {
  return { ...documentFile(path, 0), additions: 0, deletions: 0, hunks };
}
const longSnapshot = (file: FileSnapshot): AppSnapshot => ({
  ...snapshot, files: [file], total_count: 1, ai: { ...snapshot.ai, threads: [], findings: [] },
});
export const SideBySideLong: Story = {
  args: {
    snapshot: longSnapshot(sideFile(longPath, [{
      header: `@@ -0,0 +1,${longLines.length} @@`, old_start: 0, old_count: 0, new_start: 1, new_count: longLines.length, threads: [],
      lines: longLines.map((lineText, i) => ({ kind: "add" as const, old_num: null, new_num: i + 1, text: lineText, spans: [] })),
    }])),
    documents: { [longPath]: longDoc },
    viewModeOverride: "unified",
    documentMode: "side",
  },
};

/** Two hunks of an edited document: one changed line in each, three lines of context around it. */
function editedHunk(changedLine: number) {
  const from = Math.max(1, changedLine - 3);
  const to = Math.min(longLines.length, changedLine + 3);
  const lines: FileSnapshot["hunks"][number]["lines"] = [];
  for (let n = from; n <= to; n++) {
    if (n === changedLine) {
      lines.push({ kind: "del", old_num: n, new_num: null, text: `${longLines[n - 1]} (old wording)`, spans: [] });
      lines.push({ kind: "add", old_num: null, new_num: n, text: longLines[n - 1], spans: [] });
    } else {
      lines.push({ kind: "context", old_num: n, new_num: n, text: longLines[n - 1], spans: [] });
    }
  }
  return { header: `@@ -${from},${to - from + 1} +${from},${to - from + 1} @@`, old_start: from, old_count: to - from + 1, new_start: from, new_count: to - from + 1, threads: [], lines };
}
export const SideBySideEdited: Story = {
  args: {
    snapshot: longSnapshot(sideFile("docs/edited.md", [editedHunk(3), editedHunk(60)])),
    documents: { "docs/edited.md": longDoc },
    viewModeOverride: "unified",
    documentMode: "side",
  },
};
