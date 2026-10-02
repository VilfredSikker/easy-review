import type { Meta, StoryObj } from "@storybook/svelte";
import DocumentPreview from "$lib/components/DocumentPreview.svelte";

const markdown = `# Document preview

Complete document text, including unchanged paragraphs.

## GitHub formatting

- [x] Completed task
- [ ] Pending task
  - Nested list

| Name | Status |
| --- | --- |
| Preview | **Ready** |

> A quoted paragraph with ~~removed text~~ and \`inline code\`.

\`\`\`typescript
const preview = "Complete document";
\`\`\`

[External site](https://example.com) and [repository document](./guide.md).

![Repository image](./image.png)

<details><summary>More text</summary>A folded section.</details>
`;

const meta = {
  title: "Diff/DocumentPreview",
  component: DocumentPreview,
  parameters: { layout: "padded" },
  args: { path: "README.md", state: { status: "ready", text: markdown } },
} satisfies Meta<typeof DocumentPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Markdown: Story = {};
export const PlainText: Story = { args: { path: "notes.txt", state: { status: "ready", text: "Indented text\n    keeps its spaces.\n\n" + "A long line wraps within the preview. ".repeat(25) } } };
export const Loading: Story = { args: { state: { status: "loading" } } };
export const Empty: Story = { args: { state: { status: "ready", text: "" } } };
export const Unavailable: Story = { args: { state: { status: "error", message: "The file exceeds the preview size limit." }, onretry: () => {} } };
export const RemoteImage: Story = { args: { state: { status: "ready", text: "# Image sizing\n\n![Remote image](https://placehold.co/1200x600/png)\n\nText below the image." } } };
