import type { Meta, StoryObj } from "@storybook/svelte";
import ExportModal, { openExportModal } from "$lib/components/ExportModal.svelte";

// Storybook can't reach Tauri's backend, so we pass `previewOverride` to render
// canned markdown bodies. Each story opens the modal on mount.

const defaultBody = `# Review export — show-experiment-params

## packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte

### \`packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte:38\` — Question (you)
> Should this be typed against SchemaMediaProperties instead?

### \`…/MediaCombobox.svelte:40\` — Comment (you)
> Drop the underscore prefix — the param is used.

## packages/discovery-platform/src/lib/variant-warning-copy.ts

### \`…/variant-warning-copy.ts:42\` — AI finding (high · correctness)
**Fallback returns undefined when severity is missing.**
> Callers expect a string. Either narrow the type or return an empty string.
`;

const onlyUnresolvedBody = `# Review export — show-experiment-params

## packages/discovery-platform/src/lib/variant-warning-copy.ts

### \`…/variant-warning-copy.ts:42\` — AI finding (high · correctness)
**Fallback returns undefined when severity is missing.**
> Callers expect a string. Either narrow the type or return an empty string.
`;

const emptyBody = `# Review export — feature-branch
No annotations.
`;

const withAnnotationsBody = `# Review export — show-experiment-params

## packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte

### \`packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte:38\` — Question (you)
> Should this be typed against SchemaMediaProperties instead?

## UI annotations

### \`/dashboard\`
- **Pin #1** (\`button.primary\` @ (240, 380)) — Padding looks off vs design.
- **Pin #2** — Approximate location — Missing loading state.

### \`/settings\`
- **Pin #1** (\`input#email\` @ (100, 200)) — Validation hint cut off.
`;

const meta = {
  title: "Modals/ExportModal",
  component: ExportModal,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
  decorators: [
    // Force the modal open for every story render.
    () => {
      openExportModal();
      return { Component: undefined as never };
    },
  ],
} satisfies Meta<typeof ExportModal>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { previewOverride: defaultBody },
};

export const OnlyUnresolved: Story = {
  args: { previewOverride: onlyUnresolvedBody },
};

export const EmptyReview: Story = {
  args: { previewOverride: emptyBody },
};

export const WithAnnotations: Story = {
  args: { previewOverride: withAnnotationsBody },
};
