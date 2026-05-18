import type { Meta, StoryObj } from "@storybook/svelte";
import PromoteModal from "$lib/components/PromoteModal.svelte";

const meta = {
  title: "Diff/PromoteModal",
  component: PromoteModal,
  parameters: { layout: "fullscreen", backgrounds: { default: "app" } },
} satisfies Meta<typeof PromoteModal>;

export default meta;
type Story = StoryObj<typeof meta>;

const noop = () => {
  /* storybook stub */
};

export const QuestionVariant: Story = {
  args: {
    open: true,
    kind: "question",
    sourceId: "q-1737000000000-1",
    targetLineLabel:
      "packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte:144",
    initialBody:
      "Should handleExperimentOptionSelect live on the parent component instead?\n\n> **AI** replied:\n> The combobox owns comboboxOpen and uses it locally for show/hide. Lifting handleExperimentOptionSelect up would force the parent to manage that toggle too. Recommend keeping it here.",
    onSubmit: noop,
    onClose: noop,
  },
};

export const FindingVariant: Story = {
  args: {
    open: true,
    kind: "finding",
    sourceId: "finding-1",
    targetLineLabel:
      "packages/discovery-platform/src/lib/variant-warning-copy.ts:42",
    initialBody:
      "Fallback returns undefined when severity is missing — callers expect a string.\n\nWithout a guaranteed string return, downstream `.length` reads and string concatenations will crash at runtime when severity is absent from the metadata payload.",
    onSubmit: noop,
    onClose: noop,
  },
};
