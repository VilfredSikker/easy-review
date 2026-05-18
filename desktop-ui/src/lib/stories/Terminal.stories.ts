import type { Meta, StoryObj } from "@storybook/svelte";
import TerminalHarness from "./TerminalHarness.svelte";

// Storybook has no Tauri runtime, so we can't spawn a real PTY. The harness
// renders a raw xterm.js instance pre-filled with fixture bytes — enough to
// verify the visual style without invoking the Rust side.
const meta = {
  title: "Components/Terminal",
  component: TerminalHarness,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof TerminalHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WithMockedOutput: Story = {
  args: {
    fixture:
      "\x1b[1;34mvilfred@laptop\x1b[0m:\x1b[1;36m~/projects/easy-review\x1b[0m$ ls\r\n" +
      "\x1b[1;34mcrates\x1b[0m  \x1b[1;34mdesktop-ui\x1b[0m  README.md  Cargo.toml\r\n" +
      "\x1b[1;34mvilfred@laptop\x1b[0m:\x1b[1;36m~/projects/easy-review\x1b[0m$ git status\r\n" +
      "On branch \x1b[1;32mmain\x1b[0m\r\n" +
      "Your branch is up to date with 'origin/main'.\r\n\r\n" +
      "nothing to commit, working tree clean\r\n" +
      "\x1b[1;34mvilfred@laptop\x1b[0m:\x1b[1;36m~/projects/easy-review\x1b[0m$ ",
  },
};

// Visual-only mock of the new toolbar (branch label + insert-checkout button +
// close). The harness renders a static toolbar above the xterm so reviewers
// can eyeball spacing/tokens without a real PTY.
export const WithCheckoutToolbar: Story = {
  args: {
    branch: "feat/terminal-drawer",
    showToolbar: true,
    fixture:
      "\x1b[1;34mvilfred@laptop\x1b[0m:\x1b[1;36m~/projects/easy-review\x1b[0m$ ",
  },
};
